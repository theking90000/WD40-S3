#![cfg(all(feature = "grpc-client", feature = "grpc-server"))]

use std::{error::Error as StdError, time::Duration};

use craft_codec::{Compression, Config, Encryption, FrameSizes, Framing, Metadata, MetadataParts};
use futures_util::{StreamExt, TryStreamExt};
use stream_registry::{
    Error, GrpcService, LocalRegistry, RemoteRegistry, StreamPart, StreamRegistry,
    grpc::{MAX_MESSAGE_SIZE, proto},
    traits::StreamRegistry as _,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    Code,
    transport::{Endpoint, Server},
};

type TestResult<T = ()> = Result<T, Box<dyn StdError>>;

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn metadata(length: u64) -> TestResult<Metadata> {
    let config = Config::new(Framing::Fixed(16), Compression::None, Encryption::None)?;
    let mut metadata = Metadata::new(config);
    metadata.push(FrameSizes::new(length, length)?)?;
    Ok(metadata)
}

fn local(database: &sled::Db) -> TestResult<StreamRegistry> {
    Ok(StreamRegistry::Local(LocalRegistry::new(database.clone())?))
}

struct TestServer {
    endpoint: Endpoint,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<(), tonic::transport::Error>>,
}

impl TestServer {
    async fn start(registry: StreamRegistry, mut server: Server) -> TestResult<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = Endpoint::from_shared(format!("http://{}", listener.local_addr()?))?
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(2));
        let (shutdown, signal) = oneshot::channel();
        let service = GrpcService::new(registry).into_server();
        let task = tokio::spawn(server.add_service(service).serve_with_incoming_shutdown(
            TcpListenerStream::new(listener),
            async {
                let _ = signal.await;
            },
        ));
        Ok(Self {
            endpoint,
            shutdown: Some(shutdown),
            task,
        })
    }

    async fn stop(&mut self) -> TestResult {
        self.shutdown
            .take()
            .ok_or("server already stopped")?
            .send(())
            .map_err(|_| "server shutdown channel closed")?;
        tokio::time::timeout(Duration::from_secs(5), &mut self.task).await???;
        Ok(())
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn concurrent_writes_on_sixteen_tokio_workers_complete() -> TestResult {
    use std::sync::Arc;
    use tokio::{sync::Barrier, task::JoinSet};

    let mut servers = Vec::new();
    let mut clients = Vec::new();
    for _ in 0..16 {
        let db = sled::Config::new()
            .temporary(true)
            .flush_every_ms(None)
            .open()?;
        let server = TestServer::start(local(&db)?, Server::builder()).await?;
        clients.push(
            RemoteRegistry::connect(server.endpoint.clone().timeout(Duration::from_secs(10)))
                .await?,
        );
        servers.push(server);
    }
    let metadata = metadata(3)?;
    // First saturate independent databases, then one shared database/server.
    for shared in [false, true] {
        let barrier = Arc::new(Barrier::new(65));
        let mut tasks = JoinSet::new();
        let selected: Vec<_> = if shared {
            vec![clients.first().ok_or("missing client")?.clone()]
        } else {
            clients.clone()
        };
        for (number, registry) in selected.into_iter().cycle().take(64).enumerate() {
            let barrier = barrier.clone();
            let metadata = metadata.clone();
            tasks.spawn(async move {
                barrier.wait().await;
                for _ in 0..4 {
                    let file_id = number as u128;
                    let part = registry.insert(file_id, metadata.clone()).await?;
                    let stream = registry.store(vec![part, part]).await?;
                    let parts = registry
                        .retrieve_parts(&[stream])
                        .await?
                        .try_collect::<Vec<_>>()
                        .await?;
                    let expected = StreamPart {
                        file_id,
                        metadata: metadata.clone(),
                    };
                    if parts != vec![expected.clone(), expected] {
                        return Err("parallel gRPC calls changed metadata or repetitions".into());
                    }
                }
                Ok::<_, Box<dyn StdError + Send + Sync>>(())
            });
        }
        barrier.wait().await;
        tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(result) = tasks.join_next().await {
                result??;
            }
            Ok::<_, Box<dyn StdError + Send + Sync>>(())
        })
        .await?
        .map_err(|error| -> Box<dyn StdError> { error })?;
    }
    drop(clients);
    for mut server in servers {
        server.stop().await?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn roundtrip_and_relay_preserve_order_repetitions_and_empty_streams() -> TestResult {
    let db = sled::Config::new().temporary(true).open()?;
    let mut upstream = TestServer::start(local(&db)?, Server::builder()).await?;
    let upstream_client = RemoteRegistry::connect(upstream.endpoint.clone()).await?;
    let mut relay =
        TestServer::start(StreamRegistry::Remote(upstream_client), Server::builder()).await?;
    let registry = StreamRegistry::Remote(RemoteRegistry::connect(relay.endpoint.clone()).await?);
    let first = registry.insert(u128::MAX, metadata(3)?).await?;
    let second = registry.insert(42, metadata(5)?).await?;
    let a = registry.store(vec![first, second, first]).await?;
    let b = registry.store(vec![second, first]).await?;
    let empty = registry.store(Vec::new()).await?;
    let expected_first = StreamPart {
        file_id: u128::MAX,
        metadata: metadata(3)?,
    };
    let expected_second = StreamPart {
        file_id: 42,
        metadata: metadata(5)?,
    };
    let requests = vec![a, empty, b, a];
    let parts = registry.retrieve_parts(&requests).await?;
    drop(requests);
    check(
        parts.try_collect::<Vec<_>>().await?
            == vec![
                expected_first.clone(),
                expected_second.clone(),
                expected_first.clone(),
                expected_second.clone(),
                expected_first.clone(),
                expected_first.clone(),
                expected_second,
                expected_first,
            ],
        "remote/relay retrieval changed metadata, order or repetitions",
    )?;
    check(
        registry.retrieve_parts(&[]).await?.next().await.is_none(),
        "empty request produced parts",
    )?;
    check(
        registry
            .retrieve_parts(&[empty])
            .await?
            .next()
            .await
            .is_none(),
        "empty manifest produced parts",
    )?;
    check(
        matches!(
            registry.store(vec![u128::MAX]).await,
            Err(Error::StreamPartNotFound(u128::MAX))
        ),
        "relay lost the typed missing-part error",
    )?;
    drop(registry);
    relay.stop().await?;
    upstream.stop().await
}

#[tokio::test(flavor = "current_thread")]
async fn read_errors_are_lazy_typed_and_terminal() -> TestResult {
    let db = sled::Config::new().temporary(true).open()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = RemoteRegistry::connect(server.endpoint.clone()).await?;
    let first = registry.insert(41, metadata(3)?).await?;
    let second = registry.insert(42, metadata(5)?).await?;
    let stream = registry.store(vec![first, second, first]).await?;
    let mut parts = registry
        .retrieve_parts(&[stream, u128::MAX, stream])
        .await?;
    for file_id in [41, 42, 41] {
        check(
            matches!(parts.next().await, Some(Ok(part)) if part.file_id == file_id),
            "lost a part preceding a read error",
        )?;
    }
    check(
        matches!(
            parts.next().await,
            Some(Err(Error::StreamNotFound(u128::MAX)))
        ),
        "missing stream error changed",
    )?;
    check(
        parts.next().await.is_none(),
        "read continued after a missing stream",
    )?;
    check(parts.next().await.is_none(), "terminated stream restarted")?;
    db.open_tree("stream-parts")?.remove(second.to_be_bytes())?;
    let mut parts = registry.retrieve_parts(&[stream, stream]).await?;
    check(
        matches!(parts.next().await, Some(Ok(part)) if part.file_id == 41),
        "missing part failed eagerly",
    )?;
    check(
        matches!(parts.next().await, Some(Err(Error::StreamPartNotFound(id))) if id == second),
        "missing part error changed",
    )?;
    check(
        parts.next().await.is_none(),
        "read continued after a missing part",
    )?;
    db.open_tree("stream-parts")?
        .insert(first.to_be_bytes(), vec![0xff])?;
    let mut parts = registry.retrieve_parts(&[stream]).await?;
    check(
        matches!(parts.next().await, Some(Err(Error::Rpc(status))) if status.code() == Code::Internal),
        "corrupt metadata was not an internal RPC error",
    )?;
    check(
        parts.next().await.is_none(),
        "read continued after corrupt metadata",
    )?;
    drop(parts);
    server.stop().await
}

#[tokio::test(flavor = "current_thread")]
async fn overflow_remains_a_domain_error() -> TestResult {
    let db = sled::Config::new().temporary(true).open()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = RemoteRegistry::connect(server.endpoint.clone()).await?;
    let large = Metadata::from_parts(MetadataParts {
        schema_version: craft_codec::METADATA_SCHEMA_VERSION,
        version: craft_codec::FORMAT_VERSION,
        config: Config::new(Framing::Fixed(1), Compression::None, Encryption::None)?,
        frame_count: u64::MAX,
        last_frame_len: Some(1),
        raw_lengths: None,
        payload_lengths: None,
    })?;
    let large = registry.insert(41, large).await?;
    let small = registry.insert(42, metadata(3)?).await?;
    check(
        matches!(
            registry.store(vec![large, small]).await,
            Err(Error::LengthOverflow)
        ),
        "overflow lost its domain variant",
    )?;
    check(
        db.open_tree("stream-manifests")?.is_empty(),
        "failed store published a manifest",
    )?;
    server.stop().await
}

#[tokio::test(flavor = "current_thread")]
async fn rejects_invalid_wire_data_and_oversized_messages() -> TestResult {
    let db = sled::Config::new().temporary(true).open()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let mut raw =
        proto::stream_registry_client::StreamRegistryClient::connect(server.endpoint.clone())
            .await?;
    for id in [vec![], vec![0; 15], vec![0; 17]] {
        let error = raw
            .insert(proto::InsertRequest {
                file_id: id.clone(),
                metadata: postcard::to_allocvec(&metadata(3)?)?,
            })
            .await
            .err()
            .ok_or("insert accepted an invalid ID")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid insert ID status changed",
        )?;
        let error = raw
            .store(proto::StoreRequest {
                part_ids: vec![id.clone()],
            })
            .await
            .err()
            .ok_or("store accepted an invalid ID")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid store ID status changed",
        )?;
        let error = raw
            .retrieve_parts(proto::RetrievePartsRequest {
                stream_ids: vec![id],
            })
            .await
            .err()
            .ok_or("retrieve accepted an invalid ID")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid retrieve ID status changed",
        )?;
    }
    let mut bad_schema = metadata(3)?.into_parts();
    bad_schema.schema_version = 0;
    let mut trailing = postcard::to_allocvec(&metadata(3)?)?;
    trailing.push(0);
    for bytes in [vec![0xff], postcard::to_allocvec(&bad_schema)?, trailing] {
        let error = raw
            .insert(proto::InsertRequest {
                file_id: 41_u128.to_be_bytes().to_vec(),
                metadata: bytes,
            })
            .await
            .err()
            .ok_or("insert accepted invalid metadata")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid metadata status changed",
        )?;
    }
    let error = raw
        .insert(proto::InsertRequest {
            file_id: 41_u128.to_be_bytes().to_vec(),
            metadata: vec![0; MAX_MESSAGE_SIZE + 1],
        })
        .await
        .err()
        .ok_or("server accepted a message above 4 MiB")?;
    check(
        error.code() == Code::OutOfRange,
        "server decoding limit status changed",
    )?;
    check(
        db.open_tree("stream-parts")?.is_empty(),
        "invalid requests wrote metadata",
    )?;
    let registry = RemoteRegistry::connect(server.endpoint.clone()).await?;
    let error = registry
        .store(vec![0; MAX_MESSAGE_SIZE / 16 + 1])
        .await
        .err()
        .ok_or("client accepted a message above 4 MiB")?;
    // Tonic can report a local encoding failure as an HTTP/2 reset. Preserve
    // that status instead of inventing another transport error.
    check(
        matches!(error, Error::Rpc(_)),
        "encoding failure lost its RPC status",
    )?;
    check(
        db.open_tree("stream-manifests")?.is_empty(),
        "oversized call published a manifest",
    )?;
    server.stop().await
}

#[tokio::test(flavor = "current_thread")]
async fn stopped_server_returns_an_error_without_replaying_writes() -> TestResult {
    let db = sled::Config::new().temporary(true).open()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = RemoteRegistry::connect(server.endpoint.clone()).await?;
    let part = registry.insert(41, metadata(3)?).await?;
    let stream = registry.store(vec![part]).await?;
    let parts = registry.retrieve_parts(&[stream]).await?;
    parts.try_collect::<Vec<_>>().await?;
    // Tonic owns connection tasks independently of the listening task. Await
    // shutdown so an existing connection cannot serve this write.
    server.stop().await?;
    let result = tokio::time::timeout(Duration::from_secs(3), registry.store(vec![part])).await?;
    check(result.is_err(), "write succeeded after server shutdown")?;
    check(
        db.open_tree("stream-manifests")?.len() == 1,
        "failed call replayed a write",
    )?;
    Ok(())
}

#[cfg(not(feature = "grpc-tls"))]
#[tokio::test(flavor = "current_thread")]
async fn https_is_rejected_without_tls() -> TestResult {
    let result = RemoteRegistry::connect(Endpoint::from_static("https://127.0.0.1:1")).await;
    check(
        matches!(result, Err(Error::Rpc(status)) if status.code() == Code::InvalidArgument),
        "HTTPS was not rejected before connecting",
    )
}

#[cfg(feature = "cli")]
#[tokio::test(flavor = "current_thread")]
async fn cli_remote_does_not_open_the_local_database() -> TestResult {
    use clap::Parser;
    #[derive(Parser)]
    struct Args {
        #[command(flatten)]
        registry: stream_registry::cli::StreamRegistryConfig,
    }
    let db = sled::Config::new().temporary(true).open()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let args = Args::try_parse_from([
        "test",
        "--stream-registry-path",
        "/dev/null/registry",
        "--stream-registry-endpoint",
        &server.endpoint.uri().to_string(),
    ])?;
    let registry = args.registry.open().await?;
    check(
        matches!(registry, StreamRegistry::Remote(_)),
        "CLI did not select Remote",
    )?;
    registry.store(Vec::new()).await?;
    server.stop().await
}

#[cfg(feature = "grpc-tls")]
#[tokio::test(flavor = "current_thread")]
async fn tls_verifies_trust_and_server_name() -> TestResult {
    use tonic::transport::{Certificate, ClientTlsConfig, Identity, ServerTlsConfig};
    let ca = include_str!("fixtures/ca.pem");
    let identity = Identity::from_pem(
        include_str!("fixtures/server.pem"),
        include_str!("fixtures/server-key.pem"),
    );
    let db = sled::Config::new().temporary(true).open()?;
    let builder = Server::builder().tls_config(ServerTlsConfig::new().identity(identity))?;
    let mut server = TestServer::start(local(&db)?, builder).await?;
    let uri = format!(
        "https://{}",
        server
            .endpoint
            .uri()
            .authority()
            .ok_or("missing server authority")?
    );
    let endpoint = Endpoint::from_shared(uri)?
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(2));
    let client = RemoteRegistry::connect(
        endpoint.clone().tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(ca))
                .domain_name("localhost"),
        )?,
    )
    .await?;
    let part = client.insert(41, metadata(3)?).await?;
    let stream = client.store(vec![part]).await?;
    check(
        client
            .retrieve_parts(&[stream])
            .await?
            .try_collect::<Vec<_>>()
            .await?
            == vec![StreamPart {
                file_id: 41,
                metadata: metadata(3)?,
            }],
        "TLS changed the registry data",
    )?;
    let wrong_name = RemoteRegistry::connect(
        endpoint.clone().tls_config(
            ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(ca))
                .domain_name("wrong.example"),
        )?,
    )
    .await;
    check(
        matches!(wrong_name, Err(Error::Transport(_))),
        "TLS accepted a wrong hostname",
    )?;
    let untrusted = RemoteRegistry::connect(
        endpoint.tls_config(ClientTlsConfig::new().domain_name("localhost"))?,
    )
    .await;
    check(
        matches!(untrusted, Err(Error::Transport(_))),
        "TLS accepted an untrusted certificate",
    )?;
    #[cfg(feature = "cli")]
    {
        use clap::Parser;
        #[derive(Parser)]
        struct Args {
            #[command(flatten)]
            registry: stream_registry::cli::StreamRegistryConfig,
        }
        let uri = format!(
            "https://localhost:{}",
            server
                .endpoint
                .uri()
                .port_u16()
                .ok_or("missing server port")?
        );
        let args = Args::try_parse_from([
            "test",
            "--stream-registry-path",
            "/dev/null/registry",
            "--stream-registry-endpoint",
            &uri,
            "--stream-registry-ca",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ca.pem"),
        ])?;
        args.registry.open().await?.store(Vec::new()).await?;
        let incomplete = std::process::Command::new(env!("CARGO_BIN_EXE_stream-registry"))
            .args(["--tls-cert", "unused.pem"])
            .output()?;
        check(
            incomplete.status.code() == Some(2),
            "CLI accepted a certificate without a key",
        )?;
    }
    server.stop().await
}

#![cfg(all(feature = "grpc-client", feature = "grpc-server"))]
use object_registry::{
    BlobId, Error, GrpcService, LocalRegistry, Metadata, ObjectRegistry, RemoteRegistry,
    grpc::{MAX_MESSAGE_SIZE, proto},
    traits::ObjectRegistry as _,
};
use std::{error::Error as StdError, time::Duration};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    Code,
    transport::{Endpoint, Server},
};
type TestResult<T = ()> = Result<T, Box<dyn StdError + Send + Sync>>;
fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
struct TestServer {
    endpoint: Endpoint,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<(), tonic::transport::Error>>,
}

impl TestServer {
    async fn start(registry: ObjectRegistry, mut server: Server) -> TestResult<Self> {
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

fn database() -> TestResult<sled::Db> {
    Ok(sled::Config::new()
        .temporary(true)
        .flush_every_ms(None)
        .open()?)
}
fn local(db: &sled::Db) -> TestResult<ObjectRegistry> {
    Ok(ObjectRegistry::Local(LocalRegistry::new(db.clone())?))
}

async fn exercise(registry: &ObjectRegistry, key: &str) -> TestResult {
    let metadata = Metadata::from([("任意\0field".to_owned(), "value\0unchanged".to_owned())]);
    let first = registry.store(key, BlobId::MAX, metadata.clone()).await?;
    check(
        first.previous.is_none(),
        "first remote store replaced an object",
    )?;
    let first = first.version_id;
    let record = registry.head(key, Some(first)).await?;
    check(
        record.blob_id == Some(BlobId::MAX)
            && record.metadata == metadata
            && record.version_id == first,
        "head changed owned metadata or IDs",
    )?;
    let second = registry.store(key, 7, Metadata::new()).await?;
    check(
        second.previous.as_ref() == Some(&record),
        "remote store lost its previous object",
    )?;
    let second = second.version_id;
    let page = registry.list(key, None, Some(1), true).await?;
    check(
        page.records.len() == 1 && page.records.first().is_some_and(|r| r.version_id == second),
        "history order changed",
    )?;
    let cursor = page.next_cursor.ok_or("missing history cursor")?;
    let bytes = postcard::to_allocvec(&cursor)?;
    let cursor = postcard::from_bytes(&bytes)?;
    let page = registry.list(key, Some(&cursor), Some(1), true).await?;
    check(
        page.records == vec![record] && page.next_cursor.is_none(),
        "cursor did not reconstruct the remaining page",
    )?;
    let deleted = registry.delete(key, Some(first)).await?;
    check(
        deleted.previous == deleted.current
            && deleted
                .current
                .as_ref()
                .is_some_and(|record| record.version_id == second),
        "remote historical deletion changed the current snapshots",
    )?;
    check(deleted.version_id == first, "explicit deletion ID changed")?;
    registry.delete(key, Some(first)).await?;
    let deleted = registry.delete(key, None).await?;
    check(
        deleted
            .previous
            .as_ref()
            .is_some_and(|record| record.version_id == second)
            && deleted.current.is_none(),
        "remote marker lost its visible transition",
    )?;
    let marker = deleted.version_id;
    let record = registry.head(key, Some(marker)).await?;
    check(
        record.is_delete_marker && record.blob_id.is_none(),
        "delete marker changed",
    )?;
    check(
        matches!(registry.head(key, None).await, Err(Error::ObjectNotFound(value)) if value == key),
        "marker did not hide the current object",
    )
}

#[tokio::test]
async fn domain_errors_and_optional_versions_are_preserved() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = ObjectRegistry::Remote(RemoteRegistry::connect(server.endpoint.clone()).await?);
    check(
        matches!(
            registry.store("", 1, Metadata::new()).await,
            Err(Error::InvalidKey)
        ),
        "invalid key variant changed",
    )?;
    check(
        matches!(
            registry.list("\0", None, None, false).await,
            Err(Error::InvalidPrefix)
        ),
        "invalid prefix variant changed",
    )?;
    check(
        matches!(
            registry.list("", None, Some(0), false).await,
            Err(Error::InvalidLimit)
        ),
        "zero limit was accepted",
    )?;
    check(
        matches!(registry.head("absent", Some(u64::MAX)).await, Err(Error::VersionNotFound { key, version_id }) if key == "absent" && version_id == u64::MAX),
        "missing version details changed",
    )?;
    check(
        matches!(registry.head("absent", None).await, Err(Error::ObjectNotFound(key)) if key == "absent"),
        "missing object variant changed",
    )?;
    registry.store("a/1", 1, Metadata::new()).await?;
    registry.store("a/2", 2, Metadata::new()).await?;
    let cursor = registry
        .list("a/", None, Some(1), false)
        .await?
        .next_cursor
        .ok_or("missing cursor")?;
    check(
        matches!(
            registry.list("other/", Some(&cursor), Some(1), false).await,
            Err(Error::InvalidCursor)
        ),
        "cursor scope was not validated",
    )?;
    server.stop().await
}

#[tokio::test]
async fn malformed_ids_cursors_and_oversized_messages_are_rejected() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let mut raw =
        proto::object_registry_client::ObjectRegistryClient::connect(server.endpoint.clone())
            .await?;
    for id in [vec![], vec![0; 15], vec![0; 17]] {
        let error = raw
            .store(proto::StoreRequest {
                key: "key".into(),
                blob_id: id,
                metadata: Default::default(),
            })
            .await
            .err()
            .ok_or("invalid ID accepted")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid ID status changed",
        )?;
    }
    let error = raw
        .list(proto::ListRequest {
            prefix: "".into(),
            cursor: Some(vec![0xff]),
            limit: None,
            include_versions: false,
        })
        .await
        .err()
        .ok_or("malformed cursor accepted")?;
    check(
        error.code() == Code::InvalidArgument,
        "invalid cursor status changed",
    )?;
    let error = raw
        .store(proto::StoreRequest {
            key: "x".repeat(MAX_MESSAGE_SIZE + 1),
            blob_id: vec![0; 16],
            metadata: Default::default(),
        })
        .await
        .err()
        .ok_or("oversized message accepted")?;
    check(
        error.code() == Code::OutOfRange,
        "message size limit changed",
    )?;
    server.stop().await
}

#[tokio::test]
async fn roundtrip_and_relay() -> TestResult {
    let db = database()?;
    let mut upstream = TestServer::start(local(&db)?, Server::builder()).await?;
    let upstream_client = RemoteRegistry::connect(upstream.endpoint.clone()).await?;
    let mut relay =
        TestServer::start(ObjectRegistry::Remote(upstream_client), Server::builder()).await?;
    let registry = ObjectRegistry::Remote(RemoteRegistry::connect(relay.endpoint.clone()).await?);
    exercise(&registry, "relay/object").await?;
    drop(registry);
    relay.stop().await?;
    upstream.stop().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn concurrent_operations_on_sixteen_tokio_workers_complete() -> TestResult {
    use std::sync::Arc;
    use tokio::{sync::Barrier, task::JoinSet};
    let mut servers = Vec::new();
    let mut clients = Vec::new();
    for _ in 0..16 {
        let db = database()?;
        let server = TestServer::start(local(&db)?, Server::builder()).await?;
        clients.push(ObjectRegistry::Remote(
            RemoteRegistry::connect(server.endpoint.clone().timeout(Duration::from_secs(10)))
                .await?,
        ));
        servers.push(server);
    }
    for shared in [false, true] {
        let selected = if shared {
            vec![clients.first().ok_or("missing client")?.clone()]
        } else {
            clients.clone()
        };
        let barrier = Arc::new(Barrier::new(33));
        let mut tasks = JoinSet::new();
        for (number, registry) in selected.into_iter().cycle().take(32).enumerate() {
            let barrier = barrier.clone();
            tasks.spawn(async move {
                barrier.wait().await;
                exercise(&registry, &format!("load/{shared}/{number:04}/object")).await
            });
        }
        barrier.wait().await;
        tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(result) = tasks.join_next().await {
                result??;
            }
            Ok::<_, Box<dyn StdError + Send + Sync>>(())
        })
        .await??;
    }
    drop(clients);
    for mut server in servers {
        server.stop().await?;
    }
    Ok(())
}

#[cfg(not(feature = "grpc-tls"))]
#[tokio::test]
async fn https_requires_tls_feature() -> TestResult {
    check(
        matches!(RemoteRegistry::connect(Endpoint::from_static("https://127.0.0.1:1")).await,
        Err(Error::Rpc(status)) if status.code() == Code::InvalidArgument),
        "HTTPS accepted without grpc-tls",
    )
}

#[cfg(feature = "grpc-tls")]
#[tokio::test]
async fn tls_verifies_trust_and_hostname() -> TestResult {
    use tonic::transport::{Certificate, ClientTlsConfig, Identity, ServerTlsConfig};
    let ca = include_str!("../../stream-registry/tests/fixtures/ca.pem");
    let identity = Identity::from_pem(
        include_str!("../../stream-registry/tests/fixtures/server.pem"),
        include_str!("../../stream-registry/tests/fixtures/server-key.pem"),
    );
    let db = database()?;
    let mut server = TestServer::start(
        local(&db)?,
        Server::builder().tls_config(ServerTlsConfig::new().identity(identity))?,
    )
    .await?;
    let endpoint = Endpoint::from_shared(format!(
        "https://{}",
        server.endpoint.uri().authority().ok_or("no authority")?
    ))?
    .connect_timeout(Duration::from_secs(2))
    .timeout(Duration::from_secs(10));
    let registry = ObjectRegistry::Remote(
        RemoteRegistry::connect(
            endpoint.clone().tls_config(
                ClientTlsConfig::new()
                    .ca_certificate(Certificate::from_pem(ca))
                    .domain_name("localhost"),
            )?,
        )
        .await?,
    );
    exercise(&registry, "tls/object").await?;
    check(
        matches!(
            RemoteRegistry::connect(
                endpoint.clone().tls_config(
                    ClientTlsConfig::new()
                        .ca_certificate(Certificate::from_pem(ca))
                        .domain_name("wrong.example")
                )?
            )
            .await,
            Err(Error::Transport(_))
        ),
        "wrong TLS hostname accepted",
    )?;
    check(
        matches!(
            RemoteRegistry::connect(
                endpoint.tls_config(ClientTlsConfig::new().domain_name("localhost"))?
            )
            .await,
            Err(Error::Transport(_))
        ),
        "untrusted TLS certificate accepted",
    )?;
    #[cfg(feature = "cli")]
    {
        use clap::Parser;
        #[derive(Parser)]
        struct Args {
            #[command(flatten)]
            registry: object_registry::cli::ObjectRegistryConfig,
        }
        let uri = format!(
            "https://localhost:{}",
            server.endpoint.uri().port_u16().ok_or("no port")?
        );
        let args = Args::try_parse_from([
            "test",
            "--object-registry-path",
            "/dev/null/registry",
            "--object-registry-endpoint",
            &uri,
            "--object-registry-ca",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../stream-registry/tests/fixtures/ca.pem"
            ),
        ])?;
        let registry = args.registry.open().await?;
        exercise(&registry, "tls-cli/object").await?;
        let incomplete = std::process::Command::new(env!("CARGO_BIN_EXE_object-registry"))
            .args(["--tls-cert", "unused.pem"])
            .output()?;
        check(
            incomplete.status.code() == Some(2),
            "CLI accepted a TLS certificate without a key",
        )?;
    }
    server.stop().await
}

#[cfg(feature = "cli")]
#[tokio::test]
async fn cli_remote_does_not_open_sled() -> TestResult {
    use clap::Parser;
    #[derive(Parser)]
    struct Args {
        #[command(flatten)]
        registry: object_registry::cli::ObjectRegistryConfig,
    }
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let args = Args::try_parse_from([
        "test",
        "--object-registry-path",
        "/dev/null/registry",
        "--object-registry-endpoint",
        &server.endpoint.uri().to_string(),
    ])?;
    let registry = args.registry.open().await?;
    check(
        matches!(registry, ObjectRegistry::Remote(_)),
        "CLI did not select Remote",
    )?;
    exercise(&registry, "cli/object").await?;
    server.stop().await
}

#[tokio::test]
async fn stopped_server_returns_an_error_without_retry() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = ObjectRegistry::Remote(RemoteRegistry::connect(server.endpoint.clone()).await?);
    exercise(&registry, "shutdown/object").await?;
    server.stop().await?;
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        registry.store("stopped", 1, Metadata::new()),
    )
    .await?;
    check(
        matches!(result, Err(Error::Rpc(_))),
        "write succeeded after complete server shutdown",
    )
}

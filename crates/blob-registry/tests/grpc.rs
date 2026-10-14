#![cfg(all(feature = "grpc-client", feature = "grpc-server"))]
use blob_registry::{
    BlobRegistry, Error, GrpcService, LocalRegistry, RemoteRegistry, StreamSlice,
    grpc::{MAX_MESSAGE_SIZE, proto},
    traits::BlobRegistry as _,
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
use futures_util::{StreamExt, TryStreamExt};
struct TestServer {
    endpoint: Endpoint,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<(), tonic::transport::Error>>,
}

impl TestServer {
    async fn start(registry: BlobRegistry, mut server: Server) -> TestResult<Self> {
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
fn local(db: &sled::Db) -> TestResult<BlobRegistry> {
    Ok(BlobRegistry::Local(LocalRegistry::new(db.clone())?))
}

async fn exercise(registry: &BlobRegistry, _: &str) -> TestResult {
    let blob = registry.create().await?;
    registry.can_save(blob).await?;
    registry.save_part(blob, 41, 1, u128::MAX).await?;
    registry.save_part(blob, u128::MAX, 3, u128::MAX).await?;
    registry.save_part(blob, 7, 5, 2).await?;
    let selected = vec![u128::MAX, 2, u128::MAX];
    check(
        registry.finalize(blob, selected.clone()).await? == 11,
        "finalized length changed",
    )?;
    check(
        registry.finalize(blob, selected).await? == 11,
        "identical finalize was not idempotent",
    )?;
    let slices = registry
        .read(blob, 2..9)
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    check(
        slices
            == vec![
                StreamSlice {
                    stream_id: u128::MAX,
                    skip: 2,
                    length: 1,
                },
                StreamSlice {
                    stream_id: 7,
                    skip: 0,
                    length: 5,
                },
                StreamSlice {
                    stream_id: u128::MAX,
                    skip: 0,
                    length: 1,
                },
            ],
        "range plan/order/repetitions changed",
    )?;
    let empty = registry.create().await?;
    check(
        registry.finalize(empty, Vec::new()).await? == 0,
        "empty blob length changed",
    )?;
    check(
        registry.read(empty, 0..0).await?.next().await.is_none(),
        "empty range yielded slices",
    )
}

#[tokio::test]
async fn domain_errors_and_lazy_terminal_read_errors() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = BlobRegistry::Remote(RemoteRegistry::connect(server.endpoint.clone()).await?);
    check(
        matches!(
            registry.can_save(u128::MAX).await,
            Err(Error::BlobNotFound(u128::MAX))
        ),
        "missing blob variant changed",
    )?;
    let blob = registry.create().await?;
    check(
        matches!(registry.read(blob, 0..0).await, Err(Error::BlobNotFinalized(id)) if id == blob),
        "open blob accepted a read",
    )?;
    check(
        matches!(registry.finalize(blob, vec![u128::MAX]).await, Err(Error::PartNotFound { blob_id, part_id }) if blob_id == blob && part_id == u128::MAX),
        "missing part variant changed",
    )?;
    registry.save_part(blob, 41, 3, 1).await?;
    registry.save_part(blob, 42, 5, 2).await?;
    registry.save_part(blob, 99, 3, 1).await?;
    registry.can_save(blob).await?;
    registry.finalize(blob, vec![1, 2]).await?;
    check(
        matches!(registry.can_save(blob).await, Err(Error::BlobFinalized(id)) if id == blob),
        "finalized blob accepted new parts",
    )?;
    check(
        matches!(registry.save_part(blob, 100, 9, 1).await, Err(Error::BlobFinalized(id)) if id == blob),
        "finalized blob accepted part replacement",
    )?;
    check(
        matches!(
            registry.read(blob, 2..9).await,
            Err(Error::InvalidRange {
                start: 2,
                end: 9,
                length: 8
            })
        ),
        "invalid range details changed",
    )?;
    let key: Vec<_> = blob
        .to_be_bytes()
        .into_iter()
        .chain(2_u128.to_be_bytes())
        .collect();
    db.open_tree("blob-parts")?.remove(key)?;
    let mut slices = registry.read(blob, 0..8).await?;
    check(
        matches!(slices.next().await, Some(Ok(s)) if s.stream_id == 99),
        "lazy read failed eagerly",
    )?;
    check(
        matches!(slices.next().await, Some(Err(Error::PartNotFound { blob_id, part_id: 2 })) if blob_id == blob),
        "lazy error lost its IDs",
    )?;
    check(
        slices.next().await.is_none() && slices.next().await.is_none(),
        "read continued after error",
    )?;
    server.stop().await
}

#[tokio::test]
async fn invalid_ids_and_oversized_messages_are_rejected() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let mut raw =
        proto::blob_registry_client::BlobRegistryClient::connect(server.endpoint.clone()).await?;
    for id in [vec![], vec![0; 15], vec![0; 17]] {
        let error = raw
            .can_save(proto::CanSaveRequest { blob_id: id })
            .await
            .err()
            .ok_or("invalid ID accepted")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid ID status changed",
        )?;
    }
    let error = raw
        .finalize(proto::FinalizeRequest {
            blob_id: vec![0; 16],
            part_ids: vec![vec![0; MAX_MESSAGE_SIZE + 1]],
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
        TestServer::start(BlobRegistry::Remote(upstream_client), Server::builder()).await?;
    let registry = BlobRegistry::Remote(RemoteRegistry::connect(relay.endpoint.clone()).await?);
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
        clients.push(BlobRegistry::Remote(
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
    let registry = BlobRegistry::Remote(
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
            registry: blob_registry::cli::BlobRegistryConfig,
        }
        let uri = format!(
            "https://localhost:{}",
            server.endpoint.uri().port_u16().ok_or("no port")?
        );
        let args = Args::try_parse_from([
            "test",
            "--blob-registry-path",
            "/dev/null/registry",
            "--blob-registry-endpoint",
            &uri,
            "--blob-registry-ca",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../stream-registry/tests/fixtures/ca.pem"
            ),
        ])?;
        let registry = args.registry.open().await?;
        exercise(&registry, "tls-cli/object").await?;
        let incomplete = std::process::Command::new(env!("CARGO_BIN_EXE_blob-registry"))
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
        registry: blob_registry::cli::BlobRegistryConfig,
    }
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let args = Args::try_parse_from([
        "test",
        "--blob-registry-path",
        "/dev/null/registry",
        "--blob-registry-endpoint",
        &server.endpoint.uri().to_string(),
    ])?;
    let registry = args.registry.open().await?;
    check(
        matches!(registry, BlobRegistry::Remote(_)),
        "CLI did not select Remote",
    )?;
    exercise(&registry, "cli/object").await?;
    server.stop().await
}

#[tokio::test]
async fn stopped_server_returns_an_error_without_retry() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = BlobRegistry::Remote(RemoteRegistry::connect(server.endpoint.clone()).await?);
    exercise(&registry, "shutdown/object").await?;
    server.stop().await?;
    let result = tokio::time::timeout(Duration::from_secs(3), registry.create()).await?;
    check(
        matches!(result, Err(Error::Rpc(_))),
        "write succeeded after complete server shutdown",
    )
}

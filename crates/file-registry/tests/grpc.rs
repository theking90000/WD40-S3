#![cfg(all(feature = "grpc-client", feature = "grpc-server"))]
use file_registry::{
    Error, FileAccess, FileRegistry, GrpcService, LocalRegistry, RemoteRegistry,
    grpc::{MAX_MESSAGE_SIZE, proto},
    traits::FileRegistry as _,
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
    async fn start(registry: FileRegistry, mut server: Server) -> TestResult<Self> {
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
fn backend() -> TestResult<file_registry::RegistryBackend> {
    use tokio_tcp_pool::{Endpoint, Pool, Route};
    let pool = Pool::builder(Route::Direct {
        target: Endpoint::new("127.0.0.1", 1)?,
    })
    .build()?;
    Ok(file_registry::RegistryBackend::Discord(
        backends::DiscordRegistry::new(
            pool,
            vec!["https://discord.com/api/webhooks/test/token".to_owned()],
            Vec::new(),
        ),
    ))
}
fn local(db: &sled::Db) -> TestResult<FileRegistry> {
    Ok(FileRegistry::Local(LocalRegistry::new(
        db.clone(),
        backend()?,
    )?))
}
fn reference() -> backends::AccessPath {
    backends::AccessPath::Discord(backends::DiscordAccessPath::from(
        discord_webhook_storage::DiscordFile {
            id: "test-message".to_owned(),
            url: backends::DiscordReadAccessPath {
                channel_id: 1,
                attachment_id: 2,
                attachment_name: "test.bin".to_owned(),
                ex: u64::MAX,
                is: 0,
                hm: "test-signature".to_owned(),
            },
        },
    ))
}

async fn exercise(registry: &FileRegistry, _: &str) -> TestResult {
    check(
        registry.create(0, 1024).await?.next().await.is_none(),
        "zero allocation returned paths",
    )?;
    registry.store(&[]).await?;
    check(
        registry.retrieve(&[]).await?.next().await.is_none(),
        "empty retrieval returned paths",
    )?;
    let paths = registry.create(2, 1024).await?.collect::<Vec<_>>().await;
    check(paths.len() == 2, "allocation count changed")?;
    let first = paths.first().ok_or("missing first path")?.id;
    let second = paths.get(1).ok_or("missing second path")?.id;
    check(first != second, "allocated duplicate file IDs")?;
    let pending = registry
        .retrieve(&[first])
        .await?
        .next()
        .await
        .ok_or("missing pending response")?;
    check(
        matches!(pending, Err(Error::FilePending(id)) if id == first),
        "pending state changed",
    )?;
    registry
        .store(&[
            FileAccess {
                id: first,
                access_path: reference(),
            },
            FileAccess {
                id: second,
                access_path: reference(),
            },
        ])
        .await?;
    let reads = registry
        .retrieve(&[second, first, second])
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    check(
        reads.iter().map(|r| r.id).collect::<Vec<_>>() == vec![second, first, second],
        "read order/repetitions changed",
    )?;
    for read in reads {
        check(
            matches!(read.access_path, backends::ReadAccessPath::Discord(ref p) if p.ex == u64::MAX && p.attachment_name == "test.bin"),
            "backend path serialization changed",
        )?;
    }
    Ok(())
}

#[tokio::test]
async fn read_errors_are_lazy_typed_and_terminal() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = FileRegistry::Remote(RemoteRegistry::connect(server.endpoint.clone()).await?);
    let path = registry
        .create(1, 1024)
        .await?
        .next()
        .await
        .ok_or("no write path")?;
    registry
        .store(&[FileAccess {
            id: path.id,
            access_path: reference(),
        }])
        .await?;
    let mut reads = registry.retrieve(&[path.id, u128::MAX, path.id]).await?;
    check(
        matches!(reads.next().await, Some(Ok(read)) if read.id == path.id),
        "read failed eagerly",
    )?;
    check(
        matches!(
            reads.next().await,
            Some(Err(Error::FileNotFound(u128::MAX)))
        ),
        "missing file details changed",
    )?;
    check(
        reads.next().await.is_none() && reads.next().await.is_none(),
        "stream continued after error",
    )?;
    check(
        matches!(
            registry.create(1, 20_000_001).await,
            Err(Error::MissingWriteAccessPath)
        ),
        "allocation failure was not returned before exposing a stream",
    )?;
    server.stop().await
}

#[tokio::test]
async fn malformed_ids_paths_and_oversized_messages_are_rejected() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let mut raw =
        proto::file_registry_client::FileRegistryClient::connect(server.endpoint.clone()).await?;
    for id in [vec![], vec![0; 15], vec![0; 17]] {
        let error = raw
            .retrieve(proto::RetrieveRequest { ids: vec![id] })
            .await
            .err()
            .ok_or("invalid ID accepted")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid ID status changed",
        )?;
    }
    let error = raw
        .store(proto::StoreRequest {
            files: vec![proto::FileAccess {
                id: vec![0; 16],
                access_path: vec![0xff],
            }],
        })
        .await
        .err()
        .ok_or("invalid path accepted")?;
    check(
        error.code() == Code::InvalidArgument,
        "invalid path status changed",
    )?;
    let error = raw
        .create(proto::CreateRequest {
            count: u64::MAX,
            max_file_size: 1024,
        })
        .await
        .err()
        .ok_or("unbounded allocation accepted")?;
    check(
        error.code() == Code::OutOfRange || error.code() == Code::InvalidArgument,
        "allocation budget status changed",
    )?;
    let error = raw
        .store(proto::StoreRequest {
            files: vec![proto::FileAccess {
                id: vec![0; 16],
                access_path: vec![0; MAX_MESSAGE_SIZE + 1],
            }],
        })
        .await
        .err()
        .ok_or("oversized message accepted")?;
    check(
        error.code() == Code::OutOfRange,
        "message size limit changed",
    )?;
    check(db.is_empty(), "invalid requests mutated the database")?;
    server.stop().await
}

#[tokio::test]
async fn roundtrip_and_relay() -> TestResult {
    let db = database()?;
    let mut upstream = TestServer::start(local(&db)?, Server::builder()).await?;
    let upstream_client = RemoteRegistry::connect(upstream.endpoint.clone()).await?;
    let mut relay =
        TestServer::start(FileRegistry::Remote(upstream_client), Server::builder()).await?;
    let registry = FileRegistry::Remote(RemoteRegistry::connect(relay.endpoint.clone()).await?);
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
        clients.push(FileRegistry::Remote(
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
    let registry = FileRegistry::Remote(
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
            registry: file_registry::cli::FileRegistryConfig,
        }
        let uri = format!(
            "https://localhost:{}",
            server.endpoint.uri().port_u16().ok_or("no port")?
        );
        let args = Args::try_parse_from([
            "test",
            "--file-registry-path",
            "/dev/null/registry",
            "--file-registry-endpoint",
            &uri,
            "--file-registry-ca",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../stream-registry/tests/fixtures/ca.pem"
            ),
        ])?;
        let registry = args.registry.open(backend()?).await?;
        exercise(&registry, "tls-cli/object").await?;
        let incomplete = std::process::Command::new(env!("CARGO_BIN_EXE_file-registry"))
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
        registry: file_registry::cli::FileRegistryConfig,
    }
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let args = Args::try_parse_from([
        "test",
        "--file-registry-path",
        "/dev/null/registry",
        "--file-registry-endpoint",
        &server.endpoint.uri().to_string(),
    ])?;
    let registry = args.registry.open(backend()?).await?;
    check(
        matches!(registry, FileRegistry::Remote(_)),
        "CLI did not select Remote",
    )?;
    exercise(&registry, "cli/object").await?;
    server.stop().await
}

#[tokio::test]
async fn stopped_server_returns_an_error_without_retry() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = FileRegistry::Remote(RemoteRegistry::connect(server.endpoint.clone()).await?);
    exercise(&registry, "shutdown/object").await?;
    server.stop().await?;
    let result = tokio::time::timeout(Duration::from_secs(3), registry.create(1, 1024)).await?;
    check(
        matches!(result, Err(Error::Rpc(_))),
        "write succeeded after complete server shutdown",
    )
}

#![cfg(all(feature = "grpc-client", feature = "grpc-server"))]

use std::{error::Error as StdError, time::Duration};

use policy_registry::{
    Actions, Effect, Error, GrpcService, LocalRegistry, Policy, PolicyRegistry, PrincipalSelector,
    RemoteRegistry, ResourcePattern, Revision, Statement,
    grpc::{MAX_MESSAGE_SIZE, proto},
    traits::PolicyRegistry as _,
};
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
    async fn start(registry: PolicyRegistry, mut server: Server) -> TestResult<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = Endpoint::from_shared(format!("http://{}", listener.local_addr()?))?
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10));
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

fn local(db: &sled::Db) -> TestResult<PolicyRegistry> {
    Ok(PolicyRegistry::Local(LocalRegistry::new(db.clone())?))
}

fn policy(resource: &str) -> Policy {
    Policy {
        statements: vec![
            Statement {
                effect: Effect::Allow,
                actions: Actions::ALL,
                resource: ResourcePattern::Prefix(resource.to_owned()),
            },
            Statement {
                effect: Effect::Deny,
                actions: Actions::WRITE,
                resource: ResourcePattern::Exact("任意\0key".to_owned()),
            },
        ],
    }
}

async fn exercise(registry: &PolicyRegistry, id: u128) -> TestResult {
    let selector = PrincipalSelector::Specific(id);
    let original = policy("photos/");
    check(
        registry.get_policy(id).await?.is_none(),
        "absent policy was returned",
    )?;
    let first = registry.put_policy(id, original.clone()).await?;
    check(
        registry.get_policy(id).await? == Some(original.clone()),
        "policy or ID changed",
    )?;
    let attached = registry.attach(selector, id).await?;
    check(attached > first, "attachment did not increment revision")?;
    registry.attach(selector, id).await?;
    let snapshot = registry.load_policies(&[selector, selector]).await?;
    check(
        snapshot.revision >= attached && snapshot.policies == vec![(id, original)],
        "snapshot changed policies or deduplication",
    )?;
    let changed = policy("");
    registry.put_policy(id, changed.clone()).await?;
    check(
        registry.load_policies(&[selector]).await?.policies == vec![(id, changed)],
        "replacement lost binding or content",
    )?;
    registry.detach(selector, id).await?;
    check(
        registry
            .load_policies(&[selector])
            .await?
            .policies
            .is_empty(),
        "detach retained policy",
    )?;
    registry.attach(selector, id).await?;
    registry.delete_policy(id).await?;
    check(
        registry.get_policy(id).await?.is_none(),
        "deleted policy remains readable",
    )?;
    registry
        .put_policy(id, Policy { statements: vec![] })
        .await?;
    check(
        registry.get_policy(id).await? == Some(Policy { statements: vec![] }),
        "empty policy became absent",
    )?;
    check(
        registry
            .load_policies(&[selector])
            .await?
            .policies
            .is_empty(),
        "recreation restored bindings",
    )?;
    registry.delete_policy(id).await?;
    Ok(())
}

#[tokio::test]
async fn roundtrip_and_relay_preserve_all_methods() -> TestResult {
    let db = database()?;
    let mut upstream = TestServer::start(local(&db)?, Server::builder()).await?;
    let client = RemoteRegistry::connect(upstream.endpoint.clone()).await?;
    let direct = PolicyRegistry::Remote(client.clone());
    exercise(&direct, 0).await?;
    let mut relay = TestServer::start(PolicyRegistry::Remote(client), Server::builder()).await?;
    let registry = PolicyRegistry::Remote(RemoteRegistry::connect(relay.endpoint.clone()).await?);
    exercise(&registry, u128::MAX).await?;
    check(
        matches!(
            registry
                .attach(PrincipalSelector::Everyone, u128::MAX)
                .await,
            Err(Error::PolicyNotFound(u128::MAX))
        ),
        "relay changed domain error",
    )?;
    relay.stop().await?;
    upstream.stop().await
}

#[tokio::test]
async fn domain_errors_and_idempotent_revisions_are_preserved() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = PolicyRegistry::Remote(RemoteRegistry::connect(server.endpoint.clone()).await?);
    check(
        matches!(
            registry
                .attach(PrincipalSelector::Everyone, u128::MAX)
                .await,
            Err(Error::PolicyNotFound(u128::MAX))
        ),
        "missing policy details changed",
    )?;
    check(
        registry.detach(PrincipalSelector::Everyone, 0).await? == Revision(0),
        "missing detach changed revision",
    )?;
    check(
        registry.delete_policy(0).await? == Revision(0),
        "missing delete changed revision",
    )?;
    let value = policy("photos/");
    registry.put_policy(0, value.clone()).await?;
    check(
        registry.put_policy(0, value.clone()).await? == Revision(1),
        "identical put changed revision",
    )?;
    registry.attach(PrincipalSelector::Everyone, 0).await?;
    check(
        registry.attach(PrincipalSelector::Everyone, 0).await? == Revision(2),
        "repeated attach changed revision",
    )?;
    db.open_tree("policy-state")?
        .insert(b"revision", postcard::to_allocvec(&Revision(u64::MAX))?)?;
    check(
        matches!(
            registry.put_policy(0, policy("changed/")).await,
            Err(Error::RevisionOverflow)
        ),
        "revision overflow details changed",
    )?;
    check(
        registry.get_policy(0).await? == Some(value),
        "overflow changed policy",
    )?;
    check(
        registry
            .load_policies(&[PrincipalSelector::Everyone])
            .await?
            .revision
            == Revision(u64::MAX),
        "max revision changed over transport",
    )?;
    server.stop().await
}

#[tokio::test]
async fn malformed_ids_fields_enums_and_oversized_messages_are_rejected() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let mut raw =
        proto::policy_registry_client::PolicyRegistryClient::connect(server.endpoint.clone())
            .await?;
    for id in [vec![], vec![0; 15], vec![0; 17]] {
        let error = raw
            .get_policy(proto::PolicyRequest { id })
            .await
            .err()
            .ok_or("invalid ID accepted")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid ID status changed",
        )?;
    }
    for selector in [
        None,
        Some(proto::PrincipalSelector { selector: None }),
        Some(proto::PrincipalSelector {
            selector: Some(proto::principal_selector::Selector::Specific(vec![0; 15])),
        }),
    ] {
        let error = raw
            .attach(proto::BindingRequest {
                selector,
                policy_id: vec![0; 16],
            })
            .await
            .err()
            .ok_or("invalid selector accepted")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid selector status changed",
        )?;
    }
    let valid_resource = Some(proto::ResourcePattern {
        pattern: Some(proto::resource_pattern::Pattern::Prefix(String::new())),
    });
    for invalid in [
        None,
        Some(proto::Policy {
            statements: vec![proto::Statement {
                effect: 0,
                actions: vec![1],
                resource: valid_resource.clone(),
            }],
        }),
        Some(proto::Policy {
            statements: vec![proto::Statement {
                effect: 99,
                actions: vec![],
                resource: valid_resource.clone(),
            }],
        }),
        Some(proto::Policy {
            statements: vec![proto::Statement {
                effect: 1,
                actions: vec![0],
                resource: valid_resource.clone(),
            }],
        }),
        Some(proto::Policy {
            statements: vec![proto::Statement {
                effect: 1,
                actions: vec![99],
                resource: valid_resource,
            }],
        }),
        Some(proto::Policy {
            statements: vec![proto::Statement {
                effect: 1,
                actions: vec![1],
                resource: None,
            }],
        }),
        Some(proto::Policy {
            statements: vec![proto::Statement {
                effect: 1,
                actions: vec![1],
                resource: Some(proto::ResourcePattern { pattern: None }),
            }],
        }),
    ] {
        let error = raw
            .put_policy(proto::PutPolicyRequest {
                id: vec![0; 16],
                policy: invalid,
            })
            .await
            .err()
            .ok_or("invalid policy accepted")?;
        check(
            error.code() == Code::InvalidArgument,
            "invalid policy status changed",
        )?;
    }
    let error = raw
        .put_policy(proto::PutPolicyRequest {
            id: vec![0; 16],
            policy: Some(proto::Policy {
                statements: vec![proto::Statement {
                    effect: 1,
                    actions: vec![1],
                    resource: Some(proto::ResourcePattern {
                        pattern: Some(proto::resource_pattern::Pattern::Prefix(
                            "x".repeat(MAX_MESSAGE_SIZE + 1),
                        )),
                    }),
                }],
            }),
        })
        .await
        .err()
        .ok_or("oversized policy accepted")?;
    check(
        error.code() == Code::OutOfRange,
        "message size limit changed",
    )?;
    check(
        LocalRegistry::new(db)?.load_policies(&[]).await?.revision == Revision(0),
        "rejected request changed state",
    )?;
    server.stop().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 16)]
async fn concurrent_clients_on_sixteen_workers_complete() -> TestResult {
    use std::sync::Arc;
    use tokio::{sync::Barrier, task::JoinSet};
    let mut servers = Vec::new();
    let mut clients = Vec::new();
    for _ in 0..16 {
        let db = database()?;
        let server = TestServer::start(local(&db)?, Server::builder()).await?;
        clients.push(PolicyRegistry::Remote(
            RemoteRegistry::connect(server.endpoint.clone()).await?,
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
        for (id, registry) in selected.into_iter().cycle().take(32).enumerate() {
            let barrier = barrier.clone();
            tasks.spawn(async move {
                barrier.wait().await;
                exercise(&registry, id as u128).await
            });
        }
        barrier.wait().await;
        tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(result) = tasks.join_next().await {
                result??;
            }
            Ok::<_, Box<dyn StdError + Send + Sync>>(())
        })
        .await??;
    }
    for mut server in servers {
        server.stop().await?;
    }
    Ok(())
}

#[cfg(not(feature = "grpc-tls"))]
#[tokio::test]
async fn https_requires_tls_feature() -> TestResult {
    check(
        matches!(RemoteRegistry::connect(Endpoint::from_static("https://127.0.0.1:1")).await, Err(Error::Rpc(status)) if status.code() == Code::InvalidArgument),
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
    let registry = PolicyRegistry::Remote(
        RemoteRegistry::connect(
            endpoint.clone().tls_config(
                ClientTlsConfig::new()
                    .ca_certificate(Certificate::from_pem(ca))
                    .domain_name("localhost"),
            )?,
        )
        .await?,
    );
    exercise(&registry, u128::MAX).await?;
    check(
        matches!(
            RemoteRegistry::connect(
                endpoint.clone().tls_config(
                    ClientTlsConfig::new()
                        .ca_certificate(Certificate::from_pem(ca))
                        .domain_name("wrong.example"),
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
            registry: policy_registry::cli::PolicyRegistryConfig,
        }
        let uri = format!(
            "https://localhost:{}",
            server.endpoint.uri().port_u16().ok_or("no port")?
        );
        let args = Args::try_parse_from([
            "test",
            "--policy-registry-path",
            "/dev/null/registry",
            "--policy-registry-endpoint",
            &uri,
            "--policy-registry-ca",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../stream-registry/tests/fixtures/ca.pem"
            ),
        ])?;
        exercise(&args.registry.open().await?, 1).await?;
        let incomplete = std::process::Command::new(env!("CARGO_BIN_EXE_policy-registry"))
            .args(["--tls-cert", "unused.pem"])
            .output()?;
        check(
            incomplete.status.code() == Some(2),
            "CLI accepted TLS certificate without key",
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
        registry: policy_registry::cli::PolicyRegistryConfig,
    }
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let args = Args::try_parse_from([
        "test",
        "--policy-registry-path",
        "/dev/null/registry",
        "--policy-registry-endpoint",
        &server.endpoint.uri().to_string(),
    ])?;
    let registry = args.registry.open().await?;
    check(
        matches!(registry, PolicyRegistry::Remote(_)),
        "CLI did not select Remote",
    )?;
    exercise(&registry, 1).await?;
    server.stop().await
}

#[tokio::test]
async fn stopped_server_returns_an_error_without_fallback() -> TestResult {
    let db = database()?;
    let mut server = TestServer::start(local(&db)?, Server::builder()).await?;
    let registry = PolicyRegistry::Remote(RemoteRegistry::connect(server.endpoint.clone()).await?);
    exercise(&registry, 1).await?;
    server.stop().await?;
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        registry.put_policy(1, policy("stopped/")),
    )
    .await?;
    check(
        matches!(result, Err(Error::Rpc(_))),
        "write succeeded after shutdown",
    )?;
    check(
        LocalRegistry::new(db)?.get_policy(1).await?.is_none(),
        "failed remote write changed state",
    )
}

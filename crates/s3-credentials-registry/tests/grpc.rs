#![cfg(all(feature = "grpc-client", feature = "grpc-server"))]

use std::time::Duration;

use s3_credentials_registry::{
    Credential, Error, GrpcService, KeyId, KeySecret, LocalRegistry, RemoteRegistry,
    S3CredentialRegistry,
    grpc::{MAX_MESSAGE_SIZE, proto},
    traits::S3CredentialRegistry as _,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    Code,
    transport::{Endpoint, Server},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

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
    async fn start(registry: S3CredentialRegistry) -> TestResult<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = Endpoint::from_shared(format!("http://{}", listener.local_addr()?))?
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10));
        let (shutdown, signal) = oneshot::channel();
        let service = GrpcService::new(registry).into_server();
        let task = tokio::spawn(
            Server::builder()
                .add_service(service)
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = signal.await;
                }),
        );
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
            .map_err(|_| "shutdown channel closed")?;
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

async fn exercise(registry: &S3CredentialRegistry) -> TestResult {
    let id = KeyId::new("🔑".repeat(20))?;
    let credential = Credential {
        principal_id: u128::MAX,
        secret: KeySecret::new("密".repeat(40))?,
    };
    check(
        registry.get_key(&id).await?.is_none(),
        "missing key was returned",
    )?;
    registry.revoke_key(&id).await?;
    registry
        .register_key(
            id.clone(),
            credential.secret.clone(),
            credential.principal_id,
        )
        .await?;
    check(
        registry.get_key(&id).await? == Some(credential.clone()),
        "credential changed over transport",
    )?;
    for replacement in [
        credential.clone(),
        Credential {
            principal_id: 0,
            secret: KeySecret::new("s".repeat(40))?,
        },
    ] {
        check(
            matches!(
                registry.register_key(id.clone(), replacement.secret, replacement.principal_id).await,
                Err(Error::KeyAlreadyExists(existing)) if existing == id
            ),
            "typed duplicate error was lost",
        )?;
    }
    check(
        registry.get_key(&id).await? == Some(credential),
        "duplicate overwrote credential",
    )?;
    registry.revoke_key(&id).await?;
    registry.revoke_key(&id).await?;
    check(
        registry.get_key(&id).await?.is_none(),
        "revoked key was returned",
    )
}

#[tokio::test]
async fn local_remote_and_relay_have_identical_behavior() -> TestResult {
    let local = S3CredentialRegistry::Local(LocalRegistry::new(database()?)?);
    exercise(&local).await?;
    let mut upstream = TestServer::start(local).await?;
    let remote =
        S3CredentialRegistry::Remote(RemoteRegistry::connect(upstream.endpoint.clone()).await?);
    exercise(&remote).await?;
    let mut relay = TestServer::start(remote).await?;
    let relayed =
        S3CredentialRegistry::Remote(RemoteRegistry::connect(relay.endpoint.clone()).await?);
    exercise(&relayed).await?;
    relay.stop().await?;
    upstream.stop().await
}

#[tokio::test]
async fn malformed_requests_are_rejected_without_storing_credentials() -> TestResult {
    let local = S3CredentialRegistry::Local(LocalRegistry::new(database()?)?);
    let mut server = TestServer::start(local.clone()).await?;
    let channel = server.endpoint.clone().connect().await?;
    let mut client = proto::s3_credential_registry_client::S3CredentialRegistryClient::new(channel)
        .max_encoding_message_size(MAX_MESSAGE_SIZE)
        .max_decoding_message_size(MAX_MESSAGE_SIZE);
    let id = KeyId::new("A".repeat(20))?;
    for request in [
        proto::RegisterKeyRequest {
            id: "short".to_owned(),
            secret: "s".repeat(40),
            principal_id: 0_u128.to_be_bytes().to_vec(),
        },
        proto::RegisterKeyRequest {
            id: id.as_str().to_owned(),
            secret: "short".to_owned(),
            principal_id: 0_u128.to_be_bytes().to_vec(),
        },
        proto::RegisterKeyRequest {
            id: id.as_str().to_owned(),
            secret: "s".repeat(40),
            principal_id: vec![0; 15],
        },
        proto::RegisterKeyRequest {
            id: id.as_str().to_owned(),
            secret: "s".repeat(40),
            principal_id: vec![0; 17],
        },
    ] {
        let status = client
            .register_key(request)
            .await
            .err()
            .ok_or("malformed registration succeeded")?;
        check(
            status.code() == Code::InvalidArgument,
            "wrong malformed-request status",
        )?;
    }
    let status = client
        .get_key(proto::KeyRequest { id: String::new() })
        .await
        .err()
        .ok_or("malformed lookup succeeded")?;
    check(
        status.code() == Code::InvalidArgument,
        "malformed lookup status changed",
    )?;
    let status = client
        .revoke_key(proto::KeyRequest { id: String::new() })
        .await
        .err()
        .ok_or("malformed revocation succeeded")?;
    check(
        status.code() == Code::InvalidArgument,
        "malformed revoke status changed",
    )?;
    check(
        local.get_key(&id).await?.is_none(),
        "malformed request stored a credential",
    )?;
    local
        .register_key(id.clone(), KeySecret::new("s".repeat(40))?, 0)
        .await?;
    let status = client
        .register_key(proto::RegisterKeyRequest {
            id: id.as_str().to_owned(),
            secret: "x".repeat(40),
            principal_id: 1_u128.to_be_bytes().to_vec(),
        })
        .await
        .err()
        .ok_or("duplicate succeeded")?;
    check(
        status.code() == Code::AlreadyExists,
        "duplicate status changed",
    )?;
    let details = <proto::RegistryError as prost::Message>::decode(status.details())?;
    check(
        details.kind == proto::registry_error::Kind::KeyAlreadyExists as i32
            && details.key_id == id.as_str(),
        "duplicate details lost",
    )?;
    server.stop().await
}

#[cfg(feature = "cli")]
#[tokio::test]
async fn cli_remote_selection_does_not_open_local_path() -> TestResult {
    use clap::Parser;
    use s3_credentials_registry::cli::S3CredentialsRegistryConfig;

    #[derive(Parser)]
    struct Args {
        #[command(flatten)]
        registry: S3CredentialsRegistryConfig,
    }

    let mut server = TestServer::start(S3CredentialRegistry::Local(LocalRegistry::new(
        database()?
    )?))
    .await?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "wd40-credentials-cli-{}-{unique}",
        std::process::id()
    ));
    let endpoint = server.endpoint.uri().to_string();
    let args = Args::try_parse_from([
        std::ffi::OsString::from("registry"),
        std::ffi::OsString::from("--s3-credentials-registry-path"),
        path.as_os_str().to_owned(),
        std::ffi::OsString::from("--s3-credentials-registry-endpoint"),
        endpoint.into(),
    ])?;
    let registry = args.registry.open().await?;
    check(
        matches!(&registry, S3CredentialRegistry::Remote(_)),
        "CLI selected local backend",
    )?;
    exercise(&registry).await?;
    check(
        !path.exists(),
        "CLI opened a local database for remote selection",
    )?;
    server.stop().await
}

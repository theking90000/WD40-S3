#![cfg(all(feature = "grpc-client", feature = "grpc-server"))]

use std::{collections::BTreeMap, time::Duration};

use prost::Message;
use s3_multipart_registry::{
    Error, GrpcService, LocalRegistry, MultipartUpload, MultipartUploadId, RemoteRegistry,
    S3MultipartRegistry,
    grpc::{MAX_MESSAGE_SIZE, proto},
    traits::S3MultipartRegistry as _,
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

fn database() -> TestResult<sled::Db> {
    Ok(sled::Config::new()
        .temporary(true)
        .flush_every_ms(None)
        .open()?)
}

fn upload(blob_id: u128) -> MultipartUpload {
    MultipartUpload {
        blob_id,
        bucket: "bucket".to_owned(),
        key: "directory/密.txt".to_owned(),
        metadata: BTreeMap::from([
            ("content-type".to_owned(), "text/plain".to_owned()),
            ("custom".to_owned(), "valeur 密".to_owned()),
        ]),
    }
}

struct TestServer {
    endpoint: Endpoint,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<(), tonic::transport::Error>>,
}

impl TestServer {
    async fn start(registry: S3MultipartRegistry) -> TestResult<Self> {
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

async fn exercise(registry: &S3MultipartRegistry) -> TestResult {
    let absent = MultipartUploadId::new("0".repeat(32))?;
    check(
        registry.get(&absent).await?.is_none(),
        "absent upload returned",
    )?;
    registry.remove(&absent).await?;
    let original = upload(u128::MAX);
    let id = registry.create(original.clone()).await?;
    check(
        registry.get(&id).await? == Some(original.clone()),
        "transport changed upload",
    )?;
    check(
        matches!(registry.create(original.clone()).await, Err(Error::BlobAlreadyExists(value)) if value == u128::MAX),
        "typed duplicate lost",
    )?;
    check(
        registry.get(&id).await? == Some(original.clone()),
        "duplicate changed session",
    )?;
    registry.remove(&id).await?;
    registry.remove(&id).await?;
    check(
        registry.get(&id).await?.is_none(),
        "removed upload returned",
    )?;
    let replacement = registry.create(original.clone()).await?;
    check(
        registry.get(&replacement).await? == Some(original),
        "blob reuse failed",
    )?;
    registry.remove(&replacement).await?;
    Ok(())
}

#[tokio::test]
async fn local_remote_and_relay_have_identical_behavior() -> TestResult {
    let local = S3MultipartRegistry::Local(LocalRegistry::new(database()?)?);
    exercise(&local).await?;
    let mut upstream = TestServer::start(local).await?;
    let remote =
        S3MultipartRegistry::Remote(RemoteRegistry::connect(upstream.endpoint.clone()).await?);
    exercise(&remote).await?;
    let mut relay = TestServer::start(remote).await?;
    let relayed =
        S3MultipartRegistry::Remote(RemoteRegistry::connect(relay.endpoint.clone()).await?);
    exercise(&relayed).await?;
    relay.stop().await?;
    upstream.stop().await
}

#[tokio::test]
async fn malformed_requests_do_not_insert_sessions() -> TestResult {
    let database = database()?;
    let local = S3MultipartRegistry::Local(LocalRegistry::new(database.clone())?);
    let mut server = TestServer::start(local.clone()).await?;
    let channel = server.endpoint.clone().connect().await?;
    let mut client = proto::s3_multipart_registry_client::S3MultipartRegistryClient::new(channel)
        .max_encoding_message_size(MAX_MESSAGE_SIZE)
        .max_decoding_message_size(MAX_MESSAGE_SIZE);
    let status = client
        .create(proto::CreateRequest { upload: None })
        .await
        .err()
        .ok_or("missing upload accepted")?;
    check(
        status.code() == Code::InvalidArgument,
        "missing upload status changed",
    )?;
    for length in [0, 15, 17] {
        let status = client
            .create(proto::CreateRequest {
                upload: Some(proto::MultipartUpload {
                    blob_id: vec![0; length],
                    bucket: "bucket".to_owned(),
                    key: "key".to_owned(),
                    metadata: BTreeMap::new(),
                }),
            })
            .await
            .err()
            .ok_or("malformed blob ID accepted")?;
        check(
            status.code() == Code::InvalidArgument,
            "malformed blob status changed",
        )?;
    }
    for invalid in [
        String::new(),
        "a".repeat(31),
        "A".repeat(32),
        "g".repeat(32),
    ] {
        let status = client
            .get(proto::UploadRequest {
                upload_id: invalid.clone(),
            })
            .await
            .err()
            .ok_or("malformed lookup accepted")?;
        check(
            status.code() == Code::InvalidArgument,
            "lookup status changed",
        )?;
        let status = client
            .remove(proto::UploadRequest { upload_id: invalid })
            .await
            .err()
            .ok_or("malformed removal accepted")?;
        check(
            status.code() == Code::InvalidArgument,
            "removal status changed",
        )?;
    }
    check(
        database.open_tree("s3-multipart-uploads")?.is_empty(),
        "malformed request inserted upload",
    )?;
    check(
        database.open_tree("s3-multipart-blobs")?.is_empty(),
        "malformed request inserted reverse mapping",
    )?;
    let original = upload(42);
    let id = local.create(original.clone()).await?;
    let status = client
        .create(proto::CreateRequest {
            upload: Some(proto::MultipartUpload {
                blob_id: original.blob_id.to_be_bytes().to_vec(),
                bucket: original.bucket,
                key: original.key,
                metadata: original.metadata,
            }),
        })
        .await
        .err()
        .ok_or("duplicate accepted")?;
    check(
        status.code() == Code::AlreadyExists,
        "duplicate status changed",
    )?;
    let details = proto::RegistryError::decode(status.details())?;
    check(
        details.kind == proto::registry_error::Kind::BlobAlreadyExists as i32
            && details.blob_id == 42_u128.to_be_bytes().to_vec(),
        "duplicate details changed",
    )?;
    check(
        local.get(&id).await? == Some(upload(42)),
        "duplicate changed upload",
    )?;
    server.stop().await
}

#[tokio::test]
async fn oversized_request_does_not_insert_a_session() -> TestResult {
    let database = database()?;
    let mut server = TestServer::start(S3MultipartRegistry::Local(LocalRegistry::new(
        database.clone(),
    )?))
    .await?;
    let channel = server.endpoint.clone().connect().await?;
    let mut client = proto::s3_multipart_registry_client::S3MultipartRegistryClient::new(channel)
        .max_encoding_message_size(8 * 1024 * 1024);
    let status = client
        .create(proto::CreateRequest {
            upload: Some(proto::MultipartUpload {
                blob_id: 1_u128.to_be_bytes().to_vec(),
                bucket: "bucket".to_owned(),
                key: "key".to_owned(),
                metadata: BTreeMap::from([("large".to_owned(), "x".repeat(MAX_MESSAGE_SIZE))]),
            }),
        })
        .await
        .err()
        .ok_or("oversized request accepted")?;
    check(
        status.code() == Code::OutOfRange || status.code() == Code::ResourceExhausted,
        "unexpected oversized-message status",
    )?;
    check(
        database.open_tree("s3-multipart-uploads")?.is_empty(),
        "oversized request inserted session",
    )?;
    check(
        database.open_tree("s3-multipart-blobs")?.is_empty(),
        "oversized request inserted reverse mapping",
    )?;
    server.stop().await
}

#[cfg(feature = "cli")]
#[tokio::test]
async fn cli_remote_selection_does_not_open_local_path() -> TestResult {
    use clap::Parser;
    use s3_multipart_registry::cli::S3MultipartRegistryConfig;

    #[derive(Parser)]
    struct Args {
        #[command(flatten)]
        registry: S3MultipartRegistryConfig,
    }

    let mut server =
        TestServer::start(S3MultipartRegistry::Local(LocalRegistry::new(database()?)?)).await?;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "wd40-multipart-cli-{}-{unique}",
        std::process::id()
    ));
    let args = Args::try_parse_from([
        std::ffi::OsString::from("registry"),
        std::ffi::OsString::from("--s3-multipart-registry-path"),
        path.as_os_str().to_owned(),
        std::ffi::OsString::from("--s3-multipart-registry-endpoint"),
        server.endpoint.uri().to_string().into(),
    ])?;
    let registry = args.registry.open().await?;
    check(
        matches!(&registry, S3MultipartRegistry::Remote(_)),
        "CLI selected local registry",
    )?;
    exercise(&registry).await?;
    check(!path.exists(), "remote CLI selection opened local database")?;
    server.stop().await
}

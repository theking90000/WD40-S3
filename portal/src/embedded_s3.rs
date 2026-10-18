//! Optional S3 bootstrap and host routing for the standalone portal.

use std::{io, num::NonZeroUsize, sync::Arc, time::Duration};

use access_controller::AccessController;
use axum::{Router, body::Body};
use backends::cli::DiscordBackendConfig;
use blob_registry::cli::BlobRegistryConfig;
use blob_system::BlobSystem;
use file_registry::cli::FileRegistryConfig;
use hyper::{
    Request, Response, StatusCode,
    body::Incoming,
    header::HOST,
    http::uri::Authority,
    server::conn::http1,
    service::{Service, service_fn},
};
use hyper_util::{rt::TokioIo, server::graceful::GracefulShutdown, service::TowerToHyperService};
use object_registry::cli::ObjectRegistryConfig;
use policy_registry::PolicyRegistry;
use s3_adapter::{
    S3Adapter,
    cli::S3AdapterOptions,
    s3s::{host::SingleDomain, service::S3Service},
};
use s3_credentials_registry::S3CredentialRegistry;
use s3_multipart_registry::cli::S3MultipartRegistryConfig;
use stream_registry::cli::StreamRegistryConfig;
use tokio::net::TcpListener;
use tokio_tcp_pool::{Endpoint, Pool, PoolConfig, Route, rustls};

#[derive(clap::Args)]
pub struct EmbeddedS3Args {
    /// Initialize S3 storage and serve S3 requests on the portal listener.
    #[arg(long, env = "WD40_PORTAL_EMBEDDED_S3", requires = "s3_host")]
    pub embedded_s3: bool,
    /// S3 endpoint domain, also accepting bucket.<domain> requests.
    #[arg(long, env = "WD40_PORTAL_S3_HOST", requires = "embedded_s3")]
    s3_host: Option<String>,
    #[command(flatten)]
    adapter: S3AdapterOptions,
    #[command(flatten)]
    objects: ObjectRegistryConfig,
    #[command(flatten)]
    multipart: S3MultipartRegistryConfig,
    #[command(flatten)]
    blobs: BlobRegistryConfig,
    #[command(flatten)]
    streams: StreamRegistryConfig,
    #[command(flatten)]
    files: FileRegistryConfig,
    #[command(flatten)]
    backend: DiscordBackendConfig,
    #[arg(long, env = "WD40_POOL_MAX_OPEN", default_value = "16")]
    pool_max_open: NonZeroUsize,
    #[arg(long, env = "WD40_POOL_CONNECT_TIMEOUT_MS", default_value = "10000")]
    pool_connect_timeout_ms: u64,
    #[arg(long, env = "WD40_POOL_IDLE_TIMEOUT_MS", default_value = "60000")]
    pool_idle_timeout_ms: u64,
    #[arg(long, env = "WD40_POOL_MAX_LIFETIME_MS")]
    pool_max_lifetime_ms: Option<u64>,
}

impl EmbeddedS3Args {
    pub async fn build(
        self,
        credentials: S3CredentialRegistry,
        policies: PolicyRegistry,
    ) -> io::Result<EmbeddedS3> {
        let host = self.s3_host.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "--embedded-s3 requires --s3-host",
            )
        })?;
        let authority = host.parse::<Authority>().map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid S3 host: {error}"),
            )
        })?;
        let host = authority.host().to_ascii_lowercase();
        let s3_host = SingleDomain::new(&host).map_err(io::Error::other)?;
        let mut config = self.adapter.adapter_config()?;
        let budget = self.adapter.scheduler.frame_budget();
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let tls = Arc::new(tls);
        let pool_config = PoolConfig {
            max_open: self.pool_max_open.get(),
            connect_timeout: Duration::from_millis(self.pool_connect_timeout_ms),
            idle_timeout: Duration::from_millis(self.pool_idle_timeout_ms),
            max_lifetime: self.pool_max_lifetime_ms.map(Duration::from_millis),
        };
        let pool = |host| -> io::Result<Pool> {
            Pool::builder(Route::Direct {
                target: Endpoint::new(host, 443).map_err(io::Error::other)?,
            })
            .config(pool_config.clone())
            .tls(Arc::clone(&tls))
            .build()
            .map_err(io::Error::other)
        };
        let (backend, registry_backend) = self
            .backend
            .build(pool("cdn.discordapp.com")?, pool("discord.com")?);
        let blobs = BlobSystem::new(
            self.blobs.open().await.map_err(io::Error::other)?,
            self.files
                .open(registry_backend)
                .await
                .map_err(io::Error::other)?,
            self.streams.open().await.map_err(io::Error::other)?,
            backend,
        );
        blobs
            .validate_write_config(config.writer, &budget, &mut config.scheduler)
            .map_err(io::Error::other)?;
        let service = S3Adapter::new(
            config,
            credentials,
            AccessController::new(policies),
            blobs,
            self.objects.open().await.map_err(io::Error::other)?,
            self.multipart.open().await.map_err(io::Error::other)?,
            budget,
        )
        .into_service_with_host(s3_host);
        Ok(EmbeddedS3 {
            host: host.into(),
            service,
        })
    }
}

#[derive(Clone)]
pub struct EmbeddedS3 {
    host: Arc<str>,
    service: S3Service,
}

impl EmbeddedS3 {
    pub async fn serve(self, listener: TcpListener, router: Router) -> io::Result<()> {
        eprintln!(
            "Embedded S3 listening on {} for {} and its bucket subdomains",
            listener.local_addr()?,
            self.host
        );
        let portal = TowerToHyperService::new(router);
        let service = service_fn(move |request: Request<Incoming>| {
            let s3 = self.clone();
            let portal = portal.clone();
            async move {
                let host = match request_host(&request) {
                    Ok(host) => host,
                    Err(()) => {
                        let mut response = Response::new(Body::from("Invalid Host header"));
                        *response.status_mut() = StatusCode::BAD_REQUEST;
                        return Ok::<_, io::Error>(response);
                    }
                };
                let is_s3 = host.as_str() == s3.host.as_ref()
                    || host
                        .strip_suffix(s3.host.as_ref())
                        .is_some_and(|prefix| prefix.ends_with('.') && prefix.len() > 1);
                if is_s3 {
                    s3.service
                        .call(request.map(s3_adapter::s3s::Body::from))
                        .await
                        .map(|response| response.map(Body::new))
                        .map_err(io::Error::other)
                } else {
                    match portal.call(request).await {
                        Ok(response) => Ok(response),
                        Err(never) => match never {},
                    }
                }
            }
        });
        let graceful = GracefulShutdown::new();
        let shutdown = tokio::signal::ctrl_c();
        tokio::pin!(shutdown);
        let result = loop {
            tokio::select! {
                biased;
                result = &mut shutdown => break result,
                incoming = listener.accept() => {
                    let (socket, _) = match incoming {
                        Ok(incoming) => incoming,
                        Err(error) => break Err(error),
                    };
                    let connection = http1::Builder::new()
                        .serve_connection(TokioIo::new(socket), service.clone());
                    let connection = graceful.watch(connection);
                    tokio::spawn(async move {
                        if let Err(error) = connection.await {
                            eprintln!("Portal HTTP connection failed: {error}");
                        }
                    });
                }
            }
        };
        drop(listener);
        drop(service);
        graceful.shutdown().await;
        result
    }
}

/// Inspect the authority without changing the headers or URI used by SigV4.
fn request_host(request: &Request<Incoming>) -> Result<String, ()> {
    let mut hosts = request.headers().get_all(HOST).iter();
    let host = hosts.next().ok_or(())?;
    if hosts.next().is_some() {
        return Err(());
    }
    let authority = host
        .to_str()
        .map_err(|_| ())?
        .parse::<Authority>()
        .map_err(|_| ())?;
    if request
        .uri()
        .authority()
        .is_some_and(|uri_host| !uri_host.as_str().eq_ignore_ascii_case(authority.as_str()))
    {
        return Err(());
    }
    Ok(authority.host().to_ascii_lowercase())
}

use std::{error::Error, net::SocketAddr, num::NonZeroUsize, sync::Arc, time::Duration};

use backends::cli::DiscordBackendConfig;
use blob_registry::cli::BlobRegistryConfig;
use blob_system::BlobSystem;
use clap::Parser;
use file_registry::cli::FileRegistryConfig;
use http_adapter::{HttpAdapter, cli::HttpAdapterArgs};
use hyper::server::conn::http1;
use hyper_util::{rt::TokioIo, server::graceful::GracefulShutdown};
use object_registry::cli::ObjectRegistryConfig;
use stream_registry::cli::StreamRegistryConfig;
use tokio_tcp_pool::{Endpoint, Pool, PoolConfig, Route, rustls};

#[derive(Parser)]
#[command(version, about = "HTTP Adapter: create objects and stream byte ranges")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
    #[command(flatten)]
    adapter: HttpAdapterArgs,
    #[command(flatten)]
    objects: ObjectRegistryConfig,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();
    let config = args.adapter.adapter_config()?;
    let budget = args.adapter.scheduler.frame_budget();
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    let tls = Arc::new(tls);
    let pool_config = PoolConfig {
        max_open: args.pool_max_open.get(),
        connect_timeout: Duration::from_millis(args.pool_connect_timeout_ms),
        idle_timeout: Duration::from_millis(args.pool_idle_timeout_ms),
        max_lifetime: args.pool_max_lifetime_ms.map(Duration::from_millis),
    };
    let pool = |host| -> Result<Pool, Box<dyn Error + Send + Sync>> {
        Ok(Pool::builder(Route::Direct {
            target: Endpoint::new(host, 443)?,
        })
        .config(pool_config.clone())
        .tls(Arc::clone(&tls))
        .build()?)
    };
    let (backend, registry_backend) = args
        .backend
        .build(pool("cdn.discordapp.com")?, pool("discord.com")?);
    let blobs = BlobSystem::new(
        args.blobs.open().await?,
        args.files.open(registry_backend).await?,
        args.streams.open().await?,
        backend,
    );
    let adapter = HttpAdapter::new(config, args.objects.open().await?, blobs, budget)?;
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    eprintln!("HTTP Adapter listening on {}", listener.local_addr()?);
    let graceful = GracefulShutdown::new();
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            biased;
            result = &mut shutdown => { result?; break; }
            incoming = listener.accept() => {
                let (socket, _) = incoming?;
                let connection = http1::Builder::new().serve_connection(TokioIo::new(socket), adapter.clone());
                let connection = graceful.watch(connection);
                tokio::spawn(async move {
                    if let Err(error) = connection.await { eprintln!("HTTP connection failed: {error}"); }
                });
            }
        }
    }
    drop(listener);
    graceful.shutdown().await;
    Ok(())
}

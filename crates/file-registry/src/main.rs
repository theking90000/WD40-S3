use std::{error::Error, net::SocketAddr};

use clap::Parser;
use file_registry::{GrpcService, cli::FileRegistryConfig};
use tonic::transport::Server;

#[derive(Parser)]
#[command(version, about = "Serve a file registry over gRPC")]
struct Args {
    /// Address accepting incoming gRPC connections.
    #[arg(long, default_value = "127.0.0.1:50054")]
    listen: SocketAddr,

    #[command(flatten)]
    registry: FileRegistryConfig,

    #[command(flatten)]
    backend: backends::cli::DiscordBackendConfig,

    /// PEM certificate chain for incoming TLS connections.
    #[cfg(feature = "grpc-tls")]
    #[arg(long, requires = "tls_key")]
    tls_cert: Option<std::path::PathBuf>,

    /// PEM private key for incoming TLS connections.
    #[cfg(feature = "grpc-tls")]
    #[arg(long, requires = "tls_cert")]
    tls_key: Option<std::path::PathBuf>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    // Pools are only configuration until a local backend needs URL renewal.
    let roots = tokio_tcp_pool::rustls::RootCertStore::from_iter(
        webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
    );
    let mut tls = tokio_tcp_pool::rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        tokio_tcp_pool::rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    let pool = tokio_tcp_pool::Pool::builder(tokio_tcp_pool::Route::Direct {
        target: tokio_tcp_pool::Endpoint::new("discord.com", 443)?,
    })
    .tls(std::sync::Arc::new(tls))
    .build()?;
    let backend = file_registry::RegistryBackend::Discord(backends::DiscordRegistry::new(
        pool,
        args.backend.webhooks,
        args.backend.bot_tokens,
    ));
    let service = GrpcService::new(args.registry.open(backend).await?).into_server();
    let server = Server::builder();
    #[cfg(feature = "grpc-tls")]
    let server = match (&args.tls_cert, &args.tls_key) {
        (Some(cert), Some(key)) => {
            let identity =
                tonic::transport::Identity::from_pem(std::fs::read(cert)?, std::fs::read(key)?);
            server.tls_config(tonic::transport::ServerTlsConfig::new().identity(identity))?
        }
        _ => server,
    };
    let mut server = server;
    server
        .add_service(service)
        .serve_with_shutdown(args.listen, async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("failed to receive Ctrl-C: {error}");
            }
        })
        .await?;
    Ok(())
}

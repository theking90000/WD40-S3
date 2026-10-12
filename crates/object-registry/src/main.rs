use std::{error::Error, net::SocketAddr};

use clap::Parser;
use object_registry::{GrpcService, cli::ObjectRegistryConfig};
use tonic::transport::Server;

#[derive(Parser)]
#[command(version, about = "Serve a object registry over gRPC")]
struct Args {
    /// Address accepting incoming gRPC connections.
    #[arg(long, default_value = "127.0.0.1:50053")]
    listen: SocketAddr,

    #[command(flatten)]
    registry: ObjectRegistryConfig,

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
    let service = GrpcService::new(args.registry.open().await?).into_server();
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

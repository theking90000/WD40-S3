use std::error::Error;

use clap::Parser;
use hyper::server::conn::http1;
use hyper_util::{rt::TokioIo, server::graceful::GracefulShutdown};
use s3_adapter::{S3Adapter, cli::S3AdapterArgs};

#[derive(Parser)]
#[command(version, about = "Serve the S3 adapter skeleton over HTTP")]
struct Args {
    #[command(flatten)]
    adapter: S3AdapterArgs,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();
    let service = S3Adapter::new().into_service();
    let listener = tokio::net::TcpListener::bind(args.adapter.listen).await?;
    eprintln!("S3 Adapter listening on {}", listener.local_addr()?);
    let graceful = GracefulShutdown::new();
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            biased;
            result = &mut shutdown => { result?; break; }
            incoming = listener.accept() => {
                let (socket, _) = incoming?;
                let connection = http1::Builder::new().serve_connection(TokioIo::new(socket), service.clone());
                let connection = graceful.watch(connection);
                tokio::spawn(async move {
                    if let Err(error) = connection.await { eprintln!("S3 HTTP connection failed: {error}"); }
                });
            }
        }
    }
    drop(listener);
    graceful.shutdown().await;
    Ok(())
}

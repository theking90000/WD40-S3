use clap::Parser;
use portal::{Portal, cli::PortalArgs};

#[derive(Parser)]
#[command(version, about = "WD40 portal proof of concept")]
struct Args {
    #[command(flatten)]
    portal: PortalArgs,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let listener = tokio::net::TcpListener::bind(args.portal.listen).await?;
    eprintln!("Portal listening on {}", listener.local_addr()?);
    axum::serve(listener, Portal::new().into_router())
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("Cannot listen for Ctrl-C: {error}");
            }
        })
        .await
}

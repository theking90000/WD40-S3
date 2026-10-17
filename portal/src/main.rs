use clap::Parser;
use portal::{OidcConfig, Portal, cli::PortalArgs};

#[derive(Parser)]
#[command(version, about = "WD40 portal proof of concept")]
struct Args {
    #[command(flatten)]
    portal: PortalArgs,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let router = Portal::new(OidcConfig {
        issuer_url: args.portal.oidc_issuer_url,
        client_id: args.portal.oidc_client_id,
        client_secret: args.portal.oidc_client_secret,
    })
    .await?
    .into_router();
    let listener = tokio::net::TcpListener::bind(args.portal.listen).await?;
    let basename = env!("WD40_PORTAL_BASE_PATH").trim_end_matches('/');
    let router = if basename.is_empty() {
        router
    } else {
        axum::Router::new().nest_service(basename, router)
    };
    eprintln!(
        "Portal listening on {}{}/",
        listener.local_addr()?,
        basename
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("Cannot listen for Ctrl-C: {error}");
            }
        })
        .await
}

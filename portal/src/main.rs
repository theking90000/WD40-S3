use std::io;

use clap::Parser;
use portal::{OidcConfig, Portal, cli::PortalArgs};
use portal_db::{Database, DatabaseConnection, Migrator, MigratorTrait};

#[derive(Parser)]
#[command(version, about = "WD40 portal proof of concept")]
struct Args {
    #[command(flatten)]
    portal: PortalArgs,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    let db = Database::connect(args.portal.database_url.as_str())
        .await
        .map_err(|error| io::Error::other(format!("Cannot connect to portal database: {error}")))?;
    let result = serve(args.portal, &db).await;
    let close_result = db
        .close()
        .await
        .map_err(|error| io::Error::other(format!("Cannot close portal database: {error}")));
    result.and(close_result)
}

async fn serve(args: PortalArgs, db: &DatabaseConnection) -> io::Result<()> {
    Migrator::up(db, None)
        .await
        .map_err(|error| io::Error::other(format!("Cannot migrate portal database: {error}")))?;
    let router = Portal::new(
        OidcConfig {
            issuer_url: args.oidc_issuer_url,
            client_id: args.oidc_client_id,
            client_secret: args.oidc_client_secret,
        },
        db.clone(),
    )
    .await?
    .into_router();
    let listener = tokio::net::TcpListener::bind(args.listen).await?;
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

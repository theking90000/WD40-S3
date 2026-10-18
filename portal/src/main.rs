use std::io;

use clap::Parser;
use policy_registry::cli::PolicyRegistryConfig;
use portal::{OidcConfig, Portal, cli::PortalArgs};
use portal_db::{Database, DatabaseConnection, Migrator, MigratorTrait};
use s3_credentials_registry::cli::S3CredentialsRegistryConfig;

#[derive(Parser)]
#[command(version, about = "WD40 portal proof of concept")]
struct Args {
    #[command(flatten)]
    portal: PortalArgs,
    #[command(flatten)]
    credentials: S3CredentialsRegistryConfig,
    #[command(flatten)]
    policies: PolicyRegistryConfig,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    let db = Database::connect(args.portal.database_url.as_str())
        .await
        .map_err(|error| io::Error::other(format!("Cannot connect to portal database: {error}")))?;
    let result = serve(args, &db).await;
    let close_result = db
        .close()
        .await
        .map_err(|error| io::Error::other(format!("Cannot close portal database: {error}")));
    result.and(close_result)
}

async fn serve(args: Args, db: &DatabaseConnection) -> io::Result<()> {
    Migrator::up(db, None)
        .await
        .map_err(|error| io::Error::other(format!("Cannot migrate portal database: {error}")))?;
    let credentials = args.credentials.open().await.map_err(|error| {
        io::Error::other(format!("Cannot open S3 credentials registry: {error}"))
    })?;
    let policies = args
        .policies
        .open()
        .await
        .map_err(|error| io::Error::other(format!("Cannot open policy registry: {error}")))?;
    let args = args.portal;
    let router = Portal::new(
        OidcConfig {
            issuer_url: args.oidc_issuer_url,
            client_id: args.oidc_client_id,
            client_secret: args.oidc_client_secret,
        },
        db.clone(),
        credentials,
        policies,
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

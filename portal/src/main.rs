use std::io;

use clap::Parser;
use policy_registry::cli::PolicyRegistryConfig;
use portal::{OidcConfig, Portal, cli::PortalArgs};
use portal_db::{Database, DatabaseConnection, Migrator, MigratorTrait};
use s3_credentials_registry::cli::S3CredentialsRegistryConfig;

#[cfg(feature = "embedded-s3")]
mod embedded_s3;

#[derive(Parser)]
#[command(version, about = "WD40 portal proof of concept")]
struct Args {
    /// Convert keys to individual policies and clean legacy policies, then exit.
    /// Stop other portal processes first and use the same SQL and WD40 registries.
    #[arg(long)]
    migrate_credential_policies: bool,
    #[command(flatten)]
    portal: PortalArgs,
    #[command(flatten)]
    credentials: S3CredentialsRegistryConfig,
    #[command(flatten)]
    policies: PolicyRegistryConfig,
    #[cfg(feature = "embedded-s3")]
    #[command(flatten)]
    embedded_s3: embedded_s3::EmbeddedS3Args,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let args = Args::parse();
    #[cfg(feature = "embedded-s3")]
    if args.embedded_s3.s3_listen == Some(args.portal.listen) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--s3-listen must differ from --listen; omit it to share the portal listener",
        ));
    }
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
    if args.migrate_credential_policies {
        let context = portal::sync::SyncContext::new(db.clone(), credentials, policies);
        portal::sync::migrate_credential_policies(&context)
            .await
            .map_err(|error| {
                io::Error::other(format!("Cannot migrate credential policies: {error}"))
            })?;
        eprintln!("Credential policy migration completed");
        return Ok(());
    }
    #[cfg(feature = "embedded-s3")]
    let s3 = if args.embedded_s3.embedded_s3 {
        Some(
            args.embedded_s3
                .build(credentials.clone(), policies.clone())
                .await?,
        )
    } else {
        None
    };
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
    #[cfg(feature = "embedded-s3")]
    if let Some(s3) = s3 {
        return s3.serve(listener, router, basename).await;
    }
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

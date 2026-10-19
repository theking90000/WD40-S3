//! Portal options composable with `#[command(flatten)]`.

use std::net::SocketAddr;

#[derive(clap::Args)]
pub struct PortalArgs {
    #[arg(long, env = "WD40_PORTAL_LISTEN", default_value = "127.0.0.1:8080")]
    pub listen: SocketAddr,

    #[arg(
        long,
        env = "WD40_PORTAL_DATABASE_URL",
        default_value = "sqlite://portal.db?mode=rwc",
        hide_env_values = true
    )]
    pub database_url: String,

    #[arg(long, env = "WD40_PORTAL_OIDC_ISSUER_URL")]
    pub oidc_issuer_url: String,

    #[arg(long, env = "WD40_PORTAL_OIDC_CLIENT_ID")]
    pub oidc_client_id: String,

    #[arg(long, env = "WD40_PORTAL_OIDC_CLIENT_SECRET", hide_env_values = true)]
    pub oidc_client_secret: String,
}

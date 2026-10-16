//! Portal options composable with `#[command(flatten)]`.

use std::net::SocketAddr;

#[derive(clap::Args)]
pub struct PortalArgs {
    #[arg(long, env = "WD40_PORTAL_LISTEN", default_value = "127.0.0.1:8080")]
    pub listen: SocketAddr,
}

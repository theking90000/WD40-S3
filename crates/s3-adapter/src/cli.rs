//! S3 server options composable with `#[command(flatten)]`.

use std::net::SocketAddr;

#[derive(clap::Args)]
pub struct S3AdapterArgs {
    /// Address accepting incoming S3 HTTP connections.
    #[arg(long, env = "WD40_S3_LISTEN", default_value = "127.0.0.1:9000")]
    pub listen: SocketAddr,
}

//! S3 server options composable with `#[command(flatten)]`.

use std::{io, net::SocketAddr};

use blob_system::cli::{BlobWriteConfig, StreamSchedulerConfig};

use crate::S3AdapterConfig;

#[derive(clap::Args)]
pub struct S3AdapterArgs {
    /// Address accepting incoming S3 HTTP connections.
    #[arg(long, env = "WD40_S3_LISTEN", default_value = "127.0.0.1:9000")]
    pub listen: SocketAddr,
    #[command(flatten)]
    pub options: S3AdapterOptions,
}

/// Writer and scheduler options for a caller-owned HTTP server.
#[derive(clap::Args)]
pub struct S3AdapterOptions {
    #[command(flatten)]
    pub writer: BlobWriteConfig,
    #[command(flatten)]
    pub scheduler: StreamSchedulerConfig,
}

impl S3AdapterArgs {
    pub fn adapter_config(&self) -> io::Result<S3AdapterConfig> {
        self.options.adapter_config()
    }
}

impl S3AdapterOptions {
    pub fn adapter_config(&self) -> io::Result<S3AdapterConfig> {
        Ok(S3AdapterConfig {
            writer: self.writer.writer_config().map_err(io::Error::other)?,
            scheduler: self
                .scheduler
                .scheduler_config()
                .map_err(io::Error::other)?,
        })
    }
}

//! HTTP options composable with `#[command(flatten)]`.

use crate::HttpAdapterConfig;
use blob_system::cli::{BlobWriteConfig, StreamSchedulerConfig};
use std::io;

#[derive(clap::Args)]
pub struct HttpAdapterArgs {
    /// Internal object-key prefix, distinct from the public HTTP routes.
    #[arg(long, env = "WD40_HTTP_PREFIX")]
    pub prefix: String,
    #[command(flatten)]
    pub writer: BlobWriteConfig,
    #[command(flatten)]
    pub scheduler: StreamSchedulerConfig,
}

impl HttpAdapterArgs {
    pub fn adapter_config(&self) -> io::Result<HttpAdapterConfig> {
        Ok(HttpAdapterConfig {
            prefix: self.prefix.clone(),
            writer: self.writer.writer_config().map_err(io::Error::other)?,
            scheduler: self
                .scheduler
                .scheduler_config()
                .map_err(io::Error::other)?,
        })
    }
}

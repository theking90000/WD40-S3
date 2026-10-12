//! Stream registry options composable with `#[command(flatten)]`.

use std::path::PathBuf;

use crate::{Error, LocalRegistry, StreamRegistry};

/// Command-line configuration for the local stream registry.
#[derive(clap::Args)]
pub struct StreamRegistryConfig {
    /// Persistent stream registry database directory.
    #[arg(
        long,
        global = true,
        env = "WD40_STREAM_REGISTRY_PATH",
        default_value = "./data/stream-registry"
    )]
    pub stream_registry_path: PathBuf,
}

impl StreamRegistryConfig {
    /// Opens the configured database.
    pub fn open(&self) -> Result<StreamRegistry, Error> {
        Ok(StreamRegistry::Local(LocalRegistry::new(sled::open(
            &self.stream_registry_path,
        )?)?))
    }
}

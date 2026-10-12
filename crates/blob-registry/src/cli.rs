//! Blob registry options composable with `#[command(flatten)]`.

use std::path::PathBuf;

use crate::{BlobRegistry, Error, LocalRegistry};

/// Command-line configuration for the local blob registry.
#[derive(clap::Args)]
pub struct BlobRegistryConfig {
    /// Persistent blob registry database directory.
    #[arg(
        long,
        global = true,
        env = "WD40_BLOB_REGISTRY_PATH",
        default_value = "./data/blob-registry"
    )]
    pub blob_registry_path: PathBuf,
}

impl BlobRegistryConfig {
    /// Opens the configured database.
    pub fn open(&self) -> Result<BlobRegistry, Error> {
        Ok(BlobRegistry::Local(LocalRegistry::new(sled::open(
            &self.blob_registry_path,
        )?)?))
    }
}

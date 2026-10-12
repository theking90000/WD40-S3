//! File registry options composable with `#[command(flatten)]`.

use std::path::PathBuf;

use crate::{Error, FileRegistry, LocalRegistry, RegistryBackend};

/// Command-line configuration for the local file registry.
#[derive(clap::Args)]
pub struct FileRegistryConfig {
    /// Persistent file registry database directory.
    #[arg(
        long,
        global = true,
        env = "WD40_FILE_REGISTRY_PATH",
        default_value = "./data/file-registry"
    )]
    pub file_registry_path: PathBuf,
}

impl FileRegistryConfig {
    /// Opens the configured database using the supplied backend.
    pub fn open(&self, backend: RegistryBackend) -> Result<FileRegistry, Error> {
        Ok(FileRegistry::Local(LocalRegistry::new(
            sled::open(&self.file_registry_path)?,
            backend,
        )?))
    }
}

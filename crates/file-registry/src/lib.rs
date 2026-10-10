//! File registry library for WD40-S3.

pub mod error;
pub mod local;
pub mod traits;

pub use backends::{FileBackend, RegistryBackend};
pub use error::Error;
pub use local::{LocalFile, LocalRegistry};
pub use traits::{FileAccess, FileId, ReadAccessPath, WriteAccessPath};

#[derive(Clone)]
pub enum FileRegistry {
    Local(LocalRegistry),
    // Future gRPC support will add a Remote(RemoteRegistry) variant.
}

impl traits::FileRegistry for FileRegistry {
    type WriteAccessPathStream = <LocalRegistry as traits::FileRegistry>::WriteAccessPathStream;
    type ReadAccessPathStream = <LocalRegistry as traits::FileRegistry>::ReadAccessPathStream;

    async fn create(
        &self,
        count: usize,
        max_file_size: u64,
    ) -> Result<Self::WriteAccessPathStream, Error> {
        match self {
            Self::Local(registry) => traits::FileRegistry::create(registry, count, max_file_size).await,
        }
    }

    async fn retrieve(&self, ids: &[FileId]) -> Result<Self::ReadAccessPathStream, Error> {
        match self {
            Self::Local(registry) => traits::FileRegistry::retrieve(registry, ids).await,
        }
    }

    async fn store(&self, files: &[FileAccess]) -> Result<(), Error> {
        match self {
            Self::Local(registry) => traits::FileRegistry::store(registry, files).await,
        }
    }
}

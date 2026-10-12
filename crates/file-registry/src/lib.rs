//! File registry library for WD40-S3.

#[cfg(feature = "cli")]
pub mod cli;
pub mod error;
#[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
pub mod grpc;
pub mod local;
#[cfg(feature = "grpc-client")]
pub mod remote;
pub mod traits;

pub use backends::{FileBackend, RegistryBackend};
pub use error::Error;
#[cfg(feature = "grpc-server")]
pub use grpc::GrpcService;
pub use local::{LocalFile, LocalRegistry};
#[cfg(feature = "grpc-client")]
pub use remote::RemoteRegistry;
pub use traits::{FileAccess, FileId, ReadAccessPath, WriteAccessPath};

#[derive(Clone)]
pub enum FileRegistry {
    Local(LocalRegistry),
    #[cfg(feature = "grpc-client")]
    Remote(RemoteRegistry),
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
            Self::Local(registry) => {
                traits::FileRegistry::create(registry, count, max_file_size).await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::FileRegistry::create(registry, count, max_file_size).await
            }
        }
    }
    async fn retrieve(&self, ids: &[FileId]) -> Result<Self::ReadAccessPathStream, Error> {
        match self {
            Self::Local(registry) => traits::FileRegistry::retrieve(registry, ids).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::FileRegistry::retrieve(registry, ids).await,
        }
    }
    async fn store(&self, files: &[FileAccess]) -> Result<(), Error> {
        match self {
            Self::Local(registry) => traits::FileRegistry::store(registry, files).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::FileRegistry::store(registry, files).await,
        }
    }
}

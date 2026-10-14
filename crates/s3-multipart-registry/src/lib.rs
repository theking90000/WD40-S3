//! Persistent multipart upload sessions with optional gRPC transport.

#[cfg(feature = "cli")]
pub mod cli;
pub mod error;
#[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
pub mod grpc;
pub mod local;
#[cfg(feature = "grpc-client")]
pub mod remote;
pub mod traits;

pub use blob_registry::BlobId;
pub use error::Error;
pub use error::Error as RegistryError;
#[cfg(feature = "grpc-server")]
pub use grpc::GrpcService;
pub use local::LocalRegistry;
#[cfg(feature = "grpc-client")]
pub use remote::RemoteRegistry;
pub use traits::{Cursor, MultipartUpload, MultipartUploadId, PageResult, UploadRecord};

#[derive(Clone)]
pub enum S3MultipartRegistry {
    Local(LocalRegistry),
    #[cfg(feature = "grpc-client")]
    Remote(RemoteRegistry),
}

impl traits::S3MultipartRegistry for S3MultipartRegistry {
    async fn create(&self, upload: MultipartUpload) -> Result<MultipartUploadId, Error> {
        match self {
            Self::Local(registry) => traits::S3MultipartRegistry::create(registry, upload).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::S3MultipartRegistry::create(registry, upload).await,
        }
    }

    async fn get(&self, id: &MultipartUploadId) -> Result<Option<MultipartUpload>, Error> {
        match self {
            Self::Local(registry) => traits::S3MultipartRegistry::get(registry, id).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::S3MultipartRegistry::get(registry, id).await,
        }
    }

    async fn list(
        &self,
        bucket: &str,
        prefix: &str,
        cursor: Option<&Cursor>,
        limit: Option<usize>,
    ) -> Result<PageResult, Error> {
        match self {
            Self::Local(registry) => {
                traits::S3MultipartRegistry::list(registry, bucket, prefix, cursor, limit).await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::S3MultipartRegistry::list(registry, bucket, prefix, cursor, limit).await
            }
        }
    }

    async fn remove(&self, id: &MultipartUploadId) -> Result<(), Error> {
        match self {
            Self::Local(registry) => traits::S3MultipartRegistry::remove(registry, id).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::S3MultipartRegistry::remove(registry, id).await,
        }
    }
}

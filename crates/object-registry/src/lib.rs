//! Versioned object references and opaque metadata over sled.
//!
//! This registry stores no content, interprets no metadata fields and neither
//! validates nor deletes blobs. Head and list return the same complete records.

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
#[cfg(feature = "grpc-server")]
pub use grpc::GrpcService;
pub use local::LocalRegistry;
#[cfg(feature = "grpc-client")]
pub use remote::RemoteRegistry;
pub use traits::{Cursor, DeleteResult, Metadata, PageResult, RecordMetadata, VersionId};

/// Selects the object registry implementation at runtime.
#[derive(Clone)]
pub enum ObjectRegistry {
    Local(LocalRegistry),
    #[cfg(feature = "grpc-client")]
    Remote(RemoteRegistry),
}

impl traits::ObjectRegistry for ObjectRegistry {
    async fn store(
        &self,
        key: &str,
        blob_id: BlobId,
        metadata: Metadata,
    ) -> Result<VersionId, Error> {
        match self {
            Self::Local(registry) => {
                traits::ObjectRegistry::store(registry, key, blob_id, metadata).await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::ObjectRegistry::store(registry, key, blob_id, metadata).await
            }
        }
    }
    async fn head(
        &self,
        key: &str,
        version_id: Option<VersionId>,
    ) -> Result<RecordMetadata, Error> {
        match self {
            Self::Local(registry) => traits::ObjectRegistry::head(registry, key, version_id).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::ObjectRegistry::head(registry, key, version_id).await,
        }
    }
    async fn list(
        &self,
        prefix: &str,
        cursor: Option<&Cursor>,
        limit: Option<usize>,
        include_versions: bool,
    ) -> Result<PageResult, Error> {
        match self {
            Self::Local(registry) => {
                traits::ObjectRegistry::list(registry, prefix, cursor, limit, include_versions)
                    .await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::ObjectRegistry::list(registry, prefix, cursor, limit, include_versions)
                    .await
            }
        }
    }
    async fn delete(
        &self,
        key: &str,
        version_id: Option<VersionId>,
    ) -> Result<DeleteResult, Error> {
        match self {
            Self::Local(registry) => {
                traits::ObjectRegistry::delete(registry, key, version_id).await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::ObjectRegistry::delete(registry, key, version_id).await
            }
        }
    }
}

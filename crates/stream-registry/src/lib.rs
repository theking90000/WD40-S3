//! Persistent stream manifests and CRAFT metadata for WD40-S3.
//!
//! Insert immutable parts after their files are registered, then store an ordered
//! list of part IDs as a stream. The registry does not open files or check their
//! existence in the file registry. Parts may be shared and repeated in manifests.

#[cfg(feature = "cli")]
pub mod cli;
pub mod error;
pub mod local;
pub mod traits;

pub use craft_codec::Metadata;
pub use error::Error;
pub use file_registry::FileId;
pub use local::LocalRegistry;
pub use traits::{StreamId, StreamManifest, StreamPart, StreamPartId};

/// Selects the stream registry implementation at runtime.
#[derive(Clone)]
pub enum StreamRegistry {
    Local(LocalRegistry),
    // Future gRPC support will add a Remote(RemoteRegistry) variant.
}

impl traits::StreamRegistry for StreamRegistry {
    type StreamPartStream = <LocalRegistry as traits::StreamRegistry>::StreamPartStream;

    async fn insert(&self, file_id: FileId, metadata: Metadata) -> Result<StreamPartId, Error> {
        match self {
            Self::Local(registry) => {
                traits::StreamRegistry::insert(registry, file_id, metadata).await
            }
        }
    }

    async fn store(&self, parts: Vec<StreamPartId>) -> Result<StreamId, Error> {
        match self {
            Self::Local(registry) => traits::StreamRegistry::store(registry, parts).await,
        }
    }

    async fn retrieve_parts(&self, ids: &[StreamId]) -> Result<Self::StreamPartStream, Error> {
        match self {
            Self::Local(registry) => traits::StreamRegistry::retrieve_parts(registry, ids).await,
        }
    }
}

//! Persistent blob composition and byte-range read plans for WD40-S3.
//!
//! Create a blob, save stream references with their logical lengths, then
//! finalize an ordered selection of parts. The registry stores metadata only;
//! stream validation and content I/O belong to the caller.

pub mod error;
pub mod local;
pub mod traits;

use std::ops::Range;

pub use error::Error;
pub use local::LocalRegistry;
pub use stream_registry::StreamId;
pub use traits::{BlobId, BlobManifest, BlobPart, PartId, StreamSlice};

/// Selects the blob registry implementation at runtime.
#[derive(Clone)]
pub enum BlobRegistry {
    Local(LocalRegistry),
}

impl traits::BlobRegistry for BlobRegistry {
    type StreamSliceStream = <LocalRegistry as traits::BlobRegistry>::StreamSliceStream;

    async fn create(&self) -> Result<BlobId, Error> {
        match self {
            Self::Local(registry) => traits::BlobRegistry::create(registry).await,
        }
    }

    async fn can_save(&self, blob_id: BlobId, part_id: PartId) -> Result<(), Error> {
        match self {
            Self::Local(registry) => {
                traits::BlobRegistry::can_save(registry, blob_id, part_id).await
            }
        }
    }

    async fn save_part(
        &self,
        blob_id: BlobId,
        stream_id: StreamId,
        length: u64,
        part_id: PartId,
    ) -> Result<(), Error> {
        match self {
            Self::Local(registry) => {
                traits::BlobRegistry::save_part(registry, blob_id, stream_id, length, part_id).await
            }
        }
    }

    async fn finalize(&self, blob_id: BlobId, parts: Vec<PartId>) -> Result<u64, Error> {
        match self {
            Self::Local(registry) => traits::BlobRegistry::finalize(registry, blob_id, parts).await,
        }
    }

    async fn read(
        &self,
        blob_id: BlobId,
        range: Range<u64>,
    ) -> Result<Self::StreamSliceStream, Error> {
        match self {
            Self::Local(registry) => traits::BlobRegistry::read(registry, blob_id, range).await,
        }
    }
}

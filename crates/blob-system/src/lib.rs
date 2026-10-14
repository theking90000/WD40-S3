//! Multipart blob writing and range reading over stream-system.
//!
//! [`WriteBlobPart`] uses an internal `WriteStream` and registers its stream
//! reference only after successful closing. [`ReadBlob`] drives an internal
//! `ReadStream` through the blob's ordered slices. Callers use blob and part IDs;
//! successful part closing returns the registered stream ID.
//!
//! CARBON configuration, shared frame budget and the write allocator remain
//! caller-owned. Constructors start no tasks. Writes, reads, closing and
//! progression futures advance operations only when polled.

#[cfg(feature = "cli")]
pub mod cli;

mod async_read;
mod error;
mod read_blob;
mod write_part;

use std::ops::Range;

use blob_registry::traits::BlobRegistry as _;

pub use async_read::AsyncReadAdapter;
pub use blob_registry::{BlobId, BlobRegistry, PartId, StreamId};
pub use error::Error;
pub use read_blob::ReadBlob;
pub use stream_system::{
    BytesFrameAllocator, Compression, Encryption, FileBackend, FileRegistry, FrameBudget,
    ReadFrame, SchedulerConfig, StreamRegistry, StreamWriterConfig as BlobWriterConfig, Window,
};
pub use write_part::WriteBlobPart;

/// Owned, cloneable storage handles for blob readers and part writers.
/// No CARBON resources are borrowed until a reader or writer is created.
#[derive(Clone)]
pub struct BlobSystem {
    blob_registry: BlobRegistry,
    file_registry: FileRegistry,
    stream_registry: StreamRegistry,
    backend: FileBackend,
}

impl BlobSystem {
    /// Validates writer, scheduler and budget settings without performing I/O.
    pub fn validate_write_config(
        &self,
        config: BlobWriterConfig,
        budget: &FrameBudget,
        scheduler: &mut SchedulerConfig,
    ) -> Result<(), Error> {
        let frame_size = usize::try_from(config.frame_size).map_err(stream_system::Error::from)?;
        let mut allocator =
            BytesFrameAllocator::new(frame_size).map_err(stream_system::Error::from)?;
        stream_system::WriteStream::new(
            self.file_registry.clone(),
            self.stream_registry.clone(),
            self.backend.clone(),
            config,
            &mut allocator,
            budget,
            scheduler,
        )?;
        Ok(())
    }

    /// Retains storage handles without performing I/O.
    pub fn new(
        blob_registry: BlobRegistry,
        file_registry: FileRegistry,
        stream_registry: StreamRegistry,
        backend: FileBackend,
    ) -> Self {
        Self {
            blob_registry,
            file_registry,
            stream_registry,
            backend,
        }
    }

    /// Durably creates an empty, open blob with a new ID.
    pub async fn create(&self) -> Result<BlobId, Error> {
        Ok(self.blob_registry.create().await?)
    }

    /// Rejects a missing or finalized blob before allocation.
    /// This check reserves nothing; the writer checks again before registration.
    pub async fn can_save(&self, blob_id: BlobId) -> Result<(), Error> {
        self.blob_registry.can_save(blob_id).await?;
        Ok(())
    }

    /// Checks admission and creates a part writer using caller-owned resources.
    /// The returned writer owns cloned handles and does not borrow this system.
    pub async fn write_part<'a>(
        &self,
        blob_id: BlobId,
        part_id: PartId,
        config: BlobWriterConfig,
        allocator: &'a mut BytesFrameAllocator,
        budget: &'a FrameBudget,
        carbon_config: &'a mut SchedulerConfig,
    ) -> Result<WriteBlobPart<'a>, Error> {
        WriteBlobPart::new(
            self,
            blob_id,
            part_id,
            config,
            allocator,
            budget,
            carbon_config,
        )
        .await
    }

    /// Freezes the supplied ordering and returns the total plaintext length.
    /// Every selected part must already have a successfully closed writer.
    /// Repetitions and empty selections follow the blob-registry contract.
    pub async fn finalize(&self, blob_id: BlobId, parts: Vec<PartId>) -> Result<u64, Error> {
        Ok(self.blob_registry.finalize(blob_id, parts).await?)
    }

    /// Creates a lazy reader of an exclusive-end plaintext byte range.
    /// Metadata lookup and range validation begin only when it is polled.
    /// The reader borrows only the CARBON resources, not this system.
    pub fn read<'a>(
        &self,
        blob_id: BlobId,
        range: Range<u64>,
        budget: &'a FrameBudget,
        carbon_config: &'a mut SchedulerConfig,
    ) -> Result<ReadBlob<'a>, Error> {
        ReadBlob::new(self, blob_id, range, budget, carbon_config)
    }
}

use std::{future::Future, ops::Range};

use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::{Error, StreamId};

/// A 128-bit blob identifier, unique within its registry database.
pub type BlobId = u128;

/// A caller-selected part identifier, scoped to one blob.
pub type PartId = u128;

/// An immutable stream reference and its plaintext byte length.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlobPart {
    pub stream_id: StreamId,
    pub length: u64,
}

/// The final ordering of a blob's parts and its total plaintext byte length.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlobManifest {
    pub parts: Vec<PartId>,
    pub length: u64,
}

/// A portion of a stream to read, in plaintext byte positions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamSlice {
    pub stream_id: StreamId,
    pub skip: u64,
    pub length: u64,
}

pub trait BlobRegistry {
    /// An owned read plan that does not borrow the registry or request.
    type StreamSliceStream: Stream<Item = Result<StreamSlice, Error>> + Send;

    /// Generates an ID and durably creates an open, empty blob.
    fn create(&self) -> impl Future<Output = Result<BlobId, Error>> + Send;

    /// Saves a part only while the blob is open and its PartId is absent.
    /// The caller verifies the stream and supplies its plaintext byte length.
    /// Equal PartIds in different blobs are independent.
    fn save_part(
        &self,
        blob_id: BlobId,
        stream_id: StreamId,
        length: u64,
        part_id: PartId,
    ) -> impl Future<Output = Result<(), Error>> + Send;

    /// Atomically checks all references, sums their lengths and closes the blob.
    /// The supplied order and repetitions are retained. Unselected parts are
    /// excluded; an empty list finalizes an empty blob. An identical retry
    /// returns the same length; a different list cannot change a finalized blob.
    fn finalize(
        &self,
        blob_id: BlobId,
        parts: Vec<PartId>,
    ) -> impl Future<Output = Result<u64, Error>> + Send;

    /// Resolves an exclusive-end byte range in a finalized blob to stream slices.
    /// Requires start <= end <= blob length; empty ranges yield no slices.
    /// Reads metadata lazily. The first metadata error terminates the stream.
    /// No stream content is read and no stream existence check is performed.
    fn read(
        &self,
        blob_id: BlobId,
        range: Range<u64>,
    ) -> impl Future<Output = Result<Self::StreamSliceStream, Error>> + Send;
}

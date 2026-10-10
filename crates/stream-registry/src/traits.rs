use std::future::Future;

use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::{Error, FileId, Metadata};

/// A 128-bit stream identifier, unique within its registry database.
pub type StreamId = u128;

/// A 128-bit part identifier, unique within its registry database.
pub type StreamPartId = u128;

/// An immutable ordered list of parts and its total logical byte length.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StreamManifest {
    pub parts: Vec<StreamPartId>,
    pub length: u64,
}

/// One complete CRAFT object in a file, including its validated frame metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StreamPart {
    pub file_id: FileId,
    pub metadata: Metadata,
}

pub trait StreamRegistry {
    type StreamPartStream: Stream<Item = Result<StreamPart, Error>> + Send;

    /// Persists a new immutable part without checking the referenced file.
    fn insert(
        &self,
        file_id: FileId,
        metadata: Metadata,
    ) -> impl Future<Output = Result<StreamPartId, Error>> + Send;

    /// Validates all references and persists a new immutable manifest.
    /// Order and repetitions are retained; an empty list stores an empty stream.
    fn store(
        &self,
        parts: Vec<StreamPartId>,
    ) -> impl Future<Output = Result<StreamId, Error>> + Send;

    /// Resolves streams directly to their parts in one request.
    /// Lazily concatenates parts in stream and manifest order, retaining repetitions.
    /// The returned stream yields the first read error and then terminates.
    fn retrieve_parts(
        &self,
        ids: &[StreamId],
    ) -> impl Future<Output = Result<Self::StreamPartStream, Error>> + Send;
}

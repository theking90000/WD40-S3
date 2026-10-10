use backends::AccessPath;
use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::Error;

pub type FileId = u128; // 128 bits file ID.

#[derive(Clone, Serialize, Deserialize)]
pub struct WriteAccessPath {
    pub id: FileId,
    pub access_path: backends::WriteAccessPath,
}

#[derive(Serialize, Deserialize)]
pub struct ReadAccessPath {
    pub id: FileId,
    pub access_path: backends::ReadAccessPath,
}

#[derive(Serialize, Deserialize)]
pub struct FileAccess {
    pub id: FileId,
    pub access_path: AccessPath,
}

pub trait FileRegistry {
    type WriteAccessPathStream: Stream<Item = WriteAccessPath> + Send;
    type ReadAccessPathStream: Stream<Item = Result<ReadAccessPath, Error>> + Send;

    /// Allocate IDs and paths supporting payloads up to `max_file_size` bytes.
    /// Fails when the backend cannot supply the requested paths.
    fn create(
        &self,
        count: usize,
        max_file_size: u64,
    ) -> impl std::future::Future<Output = Result<Self::WriteAccessPathStream, Error>> + Send;

    fn retrieve(
        &self,
        ids: &[FileId],
    ) -> impl std::future::Future<Output = Result<Self::ReadAccessPathStream, Error>> + Send;

    fn store(
        &self,
        files: &[FileAccess],
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
}

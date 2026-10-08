use backends::AccessPath;
use futures_core::Stream;
use serde::{Deserialize, Serialize};

use crate::Error;

pub type FileId = u128; // 128 bits file ID.

#[derive(Serialize, Deserialize)]
pub struct WriteAccessPath {
    pub id: FileId,
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
    type WriteAccessPathStream: Stream<Item = WriteAccessPath>;
    type ReadAccessPathStream: Stream<Item = Result<ReadAccessPath, Error>> + Send;

    fn create(
        &self,
        count: usize,
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

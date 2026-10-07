use std::future::Future;

use futures_core::Stream;
use serde::{Deserialize, Serialize};

pub type FileId = u128; // 128 bits file ID.

#[derive(Serialize, Deserialize)]
pub struct WriteAccessPath {
    id: FileId,
}

#[derive(Serialize, Deserialize)]
pub struct ReadAccessPath {}

#[derive(Serialize, Deserialize)]
pub struct FileAccess {
    id: FileId,
}

pub trait FileRegistry {
    type WriteAccessPathStream: Stream<Item = WriteAccessPath>;
    type ReadAccessPathStream: Stream<Item = ReadAccessPath>;
    type StoreFuture: Future<Output = ()>;

    fn create(&self, count: usize) -> Self::WriteAccessPathStream;

    fn retrieve(&self, ids: &[FileId]) -> Self::ReadAccessPathStream;

    fn store(&self, files: &[FileAccess]) -> Self::StoreFuture;
}

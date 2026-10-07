use std::future::Future;

use futures_core::Stream;

pub type FileId = u128; // 128 bits file ID.

pub struct WriteAccessPath {
    pub id: FileId,
}

pub struct ReadAccessPath {}

pub struct FileAccess {
    pub id: FileId,
}

pub trait FileRegistry {
    type WriteAccessPathStream: Stream<Item = WriteAccessPath>;
    type ReadAccessPathStream: Stream<Item = ReadAccessPath>;
    type StoreFuture: Future<Output = ()>;

    fn create(&self, count: usize) -> Self::WriteAccessPathStream;

    fn retrieve(&self, ids: &[FileId]) -> Self::ReadAccessPathStream;

    fn store(&self, files: &[FileAccess]) -> Self::StoreFuture;
}

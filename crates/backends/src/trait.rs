use std::future::Future;

use futures_core::Stream;
use futures_io::{AsyncRead, AsyncWrite};
use serde::{Serialize, de::DeserializeOwned};

use crate::Error;

pub trait FileBackend: Clone + Send {
    type ReadAccessPath;
    type WriteAccessPath;
    type ReadFile: AsyncRead + Unpin;
    type WriteFile: AsyncWrite + Unpin;

    fn write(
        &self,
        path: &Self::WriteAccessPath,
    ) -> impl Future<Output = Result<Self::WriteFile, Error>>;

    fn read(
        &self,
        path: &Self::ReadAccessPath,
        start: usize,
        end: Option<usize>,
    ) -> impl Future<Output = Result<Self::ReadFile, Error>>;
}

pub trait Expired {
    fn is_expired(&self) -> bool;
}

pub trait RegistryBackend: Clone + Send + Sync {
    type AccessPath: Expired + Serialize + DeserializeOwned;
    type WriteAccessPath: Serialize + DeserializeOwned;
    type ReadAccessPath: Serialize + DeserializeOwned;

    /// Select paths accepting file payloads up to `max_file_size` bytes.
    /// Unsupported sizes or unavailable destinations yield fewer than `count` paths.
    fn write(
        &self,
        count: usize,
        max_file_size: u64,
    ) -> impl Stream<Item = Self::WriteAccessPath> + Send;

    fn read<'stream>(
        &'stream self,
        access_paths: &'stream [Self::AccessPath],
    ) -> impl Stream<Item = Result<Self::ReadAccessPath, Error>> + Send + 'stream;
}

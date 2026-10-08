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

pub trait RegistryBackend: Clone + Send {
    type AccessPath: Expired + Serialize + DeserializeOwned;
    type WriteAccessPath: Serialize + DeserializeOwned;
    type ReadAccessPath: Serialize + DeserializeOwned;

    fn write(&self, count: usize) -> impl Stream<Item = Self::WriteAccessPath>;

    fn read<'stream>(
        &'stream self,
        access_paths: &'stream [Self::AccessPath],
    ) -> impl Stream<Item = Result<Self::ReadAccessPath, Error>> + Send + 'stream;
}

use futures_core::Stream;
use serde::{Serialize, de::DeserializeOwned};

use crate::Error;

pub trait FileBackend {}

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
    ) -> impl Stream<Item = Result<Self::ReadAccessPath, Error>> + 'stream;
}

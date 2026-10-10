use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, Mutex as MapMutex, Weak},
};

use backends::{AccessPath, Expired, RegistryBackend, traits::RegistryBackend as _};
use futures_core::Stream;
use futures_util::{StreamExt, lock::Mutex, stream};
use serde::{Deserialize, Serialize};
use sled::{
    Batch, Db, Tree,
    transaction::{TransactionError, Transactional},
};

use crate::{Error, FileAccess, FileId, ReadAccessPath, WriteAccessPath, traits::FileRegistry};

type FileLocks = Arc<MapMutex<HashMap<FileId, Weak<Mutex<()>>>>>;

struct FileLock {
    id: FileId,
    mutex: Arc<Mutex<()>>,
    locks: FileLocks,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let mut locks = match self.locks.lock() {
            Ok(locks) => locks,
            Err(error) => error.into_inner(),
        };
        if Arc::strong_count(&self.mutex) == 1 {
            locks.remove(&self.id);
        }
    }
}

#[derive(Clone)]
pub struct LocalRegistry {
    database: Db,
    read_cache: Tree,
    registry: RegistryBackend,
    file_locks: FileLocks,
}

#[derive(Serialize, Deserialize)]
pub enum LocalFile {
    Pending,
    Available(AccessPath),
}

impl LocalRegistry {
    pub fn new(database: Db, registry: RegistryBackend) -> Result<Self, Error> {
        let read_cache = database.open_tree("read-cache")?;
        Ok(Self {
            database,
            read_cache,
            registry,
            file_locks: Arc::new(MapMutex::new(HashMap::new())),
        })
    }

    fn file_lock(&self, id: FileId) -> FileLock {
        let mut locks = match self.file_locks.lock() {
            Ok(locks) => locks,
            Err(error) => error.into_inner(),
        };
        let mutex = match locks.get(&id).and_then(Weak::upgrade) {
            Some(mutex) => mutex,
            None => {
                let mutex = Arc::new(Mutex::new(()));
                locks.insert(id, Arc::downgrade(&mutex));
                mutex
            }
        };
        FileLock {
            id,
            mutex,
            locks: Arc::clone(&self.file_locks),
        }
    }

    async fn retrieve_one(&self, id: FileId) -> Result<ReadAccessPath, Error> {
        let lock = self.file_lock(id);
        let _guard = lock.mutex.lock().await;
        let key = id.to_be_bytes();

        if let Some(data) = self.read_cache.get(key)? {
            let access_path: backends::ReadAccessPath = postcard::from_bytes(&data)?;
            if !access_path.is_expired() {
                return Ok(ReadAccessPath { id, access_path });
            }
        }

        let data = self.database.get(key)?.ok_or(Error::FileNotFound(id))?;
        let access_path = match postcard::from_bytes::<LocalFile>(&data)? {
            LocalFile::Pending => return Err(Error::FilePending(id)),
            LocalFile::Available(access_path) => access_path,
        };

        let access_paths = [access_path];
        let mut paths = Box::pin(self.registry.read(&access_paths));
        let access_path = paths
            .next()
            .await
            .ok_or(Error::MissingReadAccessPath(id))??;
        let data = postcard::to_allocvec(&access_path)?;
        self.read_cache.insert(key, data)?;
        Ok(ReadAccessPath { id, access_path })
    }
}

impl FileRegistry for LocalRegistry {
    type WriteAccessPathStream = stream::Iter<std::vec::IntoIter<WriteAccessPath>>;
    type ReadAccessPathStream = Pin<Box<dyn Stream<Item = Result<ReadAccessPath, Error>> + Send>>;

    async fn create(&self, count: usize) -> Result<Self::WriteAccessPathStream, Error> {
        let mut paths = Vec::new();
        if count == 0 {
            return Ok(stream::iter(paths));
        }

        let data = postcard::to_allocvec(&LocalFile::Pending)?;
        let mut batch = Batch::default();
        for _ in 0..count {
            let id = FileId::from(self.database.generate_id()?);
            batch.insert(id.to_be_bytes().to_vec(), data.clone());
            paths.push(WriteAccessPath { id });
        }

        self.database.apply_batch(batch)?;
        self.database.flush_async().await?;
        Ok(stream::iter(paths))
    }

    async fn retrieve(&self, ids: &[FileId]) -> Result<Self::ReadAccessPathStream, Error> {
        let registry = self.clone();
        Ok(Box::pin(stream::iter(ids.to_vec()).then(move |id| {
            let registry = registry.clone();
            async move { registry.retrieve_one(id).await }
        })))
    }

    async fn store(&self, files: &[FileAccess]) -> Result<(), Error> {
        if files.is_empty() {
            return Ok(());
        }

        let mut batch = Batch::default();
        let mut cache_batch = Batch::default();
        for file in files {
            let data = postcard::to_allocvec(&LocalFile::Available(file.access_path.clone()))?;
            batch.insert(file.id.to_be_bytes().to_vec(), data);
            cache_batch.remove(file.id.to_be_bytes().to_vec());
        }

        let mut ids: Vec<_> = files.iter().map(|file| file.id).collect();
        ids.sort_unstable();
        ids.dedup();
        let locks: Vec<_> = ids.into_iter().map(|id| self.file_lock(id)).collect();
        let mut guards = Vec::new();
        for lock in &locks {
            guards.push(lock.mutex.lock().await);
        }

        (&*self.database, &self.read_cache)
            .transaction(|(database, cache)| {
                database.apply_batch(&batch)?;
                cache.apply_batch(&cache_batch)?;
                Ok::<(), sled::transaction::ConflictableTransactionError<sled::Error>>(())
            })
            .map_err(|error| match error {
                TransactionError::Abort(error) | TransactionError::Storage(error) => {
                    Error::Database(error)
                }
            })?;
        self.database.flush_async().await?;
        Ok(())
    }
}

use backends::{AccessPath, RegistryBackend};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use sled::{Batch, Db};

use crate::{Error, FileAccess, FileId, FileRegistry, ReadAccessPath, WriteAccessPath};

#[derive(Clone)]
pub struct LocalRegistry {
    database: Db,
    registry: RegistryBackend,
}

#[derive(Serialize, Deserialize)]
pub enum LocalFile {
    Pending,
    Available(AccessPath),
}

impl LocalRegistry {
    pub fn new(database: Db, registry: RegistryBackend) -> Self {
        Self { database, registry }
    }
}

impl FileRegistry for LocalRegistry {
    type WriteAccessPathStream = stream::Iter<std::vec::IntoIter<WriteAccessPath>>;
    type ReadAccessPathStream = stream::Iter<std::vec::IntoIter<ReadAccessPath>>;

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
        let mut paths = Vec::new();
        for &id in ids {
            let data = self
                .database
                .get(id.to_be_bytes())?
                .ok_or(Error::FileNotFound(id))?;
            match postcard::from_bytes::<LocalFile>(&data)? {
                LocalFile::Pending => return Err(Error::FilePending(id)),
                LocalFile::Available(_) => paths.push(ReadAccessPath {}),
            }
        }
        Ok(stream::iter(paths))
    }

    async fn store(&self, files: &[FileAccess]) -> Result<(), Error> {
        if files.is_empty() {
            return Ok(());
        }

        let mut batch = Batch::default();
        for file in files {
            let data = postcard::to_allocvec(&LocalFile::Available(file.access_path.clone()))?;
            batch.insert(file.id.to_be_bytes().to_vec(), data);
        }

        self.database.apply_batch(batch)?;
        self.database.flush_async().await?;
        Ok(())
    }
}

use std::pin::Pin;

use futures_core::Stream;
use futures_util::{StreamExt, stream};
use sled::{
    Db, Tree,
    transaction::{ConflictableTransactionError, TransactionError, Transactional},
};

use crate::{
    Error, FileId, Metadata, StreamId, StreamManifest, StreamPart, StreamPartId,
    traits::StreamRegistry,
};

/// A registry with separate sled trees for immutable parts and manifests.
/// Clones share the database. Successful writes await its durable flush.
#[derive(Clone)]
pub struct LocalRegistry {
    database: Db,
    parts: Tree,
    manifests: Tree,
}

impl LocalRegistry {
    /// Uses an existing caller-owned database, without opening a file registry.
    pub fn new(database: Db) -> Result<Self, Error> {
        let parts = database.open_tree("stream-parts")?;
        let manifests = database.open_tree("stream-manifests")?;
        Ok(Self {
            database,
            parts,
            manifests,
        })
    }

    fn get_part(&self, id: StreamPartId) -> Result<StreamPart, Error> {
        let data = self
            .parts
            .get(id.to_be_bytes())?
            .ok_or(Error::StreamPartNotFound(id))?;
        Ok(postcard::from_bytes(&data)?)
    }

    fn get_manifest(&self, id: StreamId) -> Result<StreamManifest, Error> {
        let data = self
            .manifests
            .get(id.to_be_bytes())?
            .ok_or(Error::StreamNotFound(id))?;
        Ok(postcard::from_bytes(&data)?)
    }
}

fn transaction_error(error: TransactionError<Error>) -> Error {
    match error {
        TransactionError::Abort(error) => error,
        TransactionError::Storage(error) => Error::Database(error),
    }
}

impl StreamRegistry for LocalRegistry {
    type StreamPartStream = Pin<Box<dyn Stream<Item = Result<StreamPart, Error>> + Send>>;

    async fn insert(&self, file_id: FileId, metadata: Metadata) -> Result<StreamPartId, Error> {
        let id = StreamPartId::from(self.database.generate_id()?);
        let key = id.to_be_bytes();
        let data = postcard::to_allocvec(&StreamPart { file_id, metadata })?;
        self.parts
            .transaction(|parts| {
                if parts.get(key)?.is_some() {
                    return Err(ConflictableTransactionError::Abort(
                        Error::StreamPartAlreadyExists(id),
                    ));
                }
                parts.insert(key.as_slice(), data.as_slice())?;
                Ok(())
            })
            .map_err(transaction_error)?;
        self.database.flush_async().await?;
        Ok(id)
    }

    async fn store(&self, parts: Vec<StreamPartId>) -> Result<StreamId, Error> {
        let id = StreamId::from(self.database.generate_id()?);
        let key = id.to_be_bytes();
        (&self.parts, &self.manifests)
            .transaction(|(part_tree, manifests)| {
                if manifests.get(key)?.is_some() {
                    return Err(ConflictableTransactionError::Abort(
                        Error::StreamAlreadyExists(id),
                    ));
                }
                let mut length = 0_u64;
                for &part_id in &parts {
                    let data = part_tree.get(part_id.to_be_bytes())?.ok_or(
                        ConflictableTransactionError::Abort(Error::StreamPartNotFound(part_id)),
                    )?;
                    let part: StreamPart = postcard::from_bytes(&data).map_err(|error| {
                        ConflictableTransactionError::Abort(Error::Serialization(error))
                    })?;
                    length = length
                        .checked_add(part.metadata.logical_len())
                        .ok_or(ConflictableTransactionError::Abort(Error::LengthOverflow))?;
                }
                let data = postcard::to_allocvec(&StreamManifest {
                    parts: parts.clone(),
                    length,
                })
                .map_err(|error| {
                    ConflictableTransactionError::Abort(Error::Serialization(error))
                })?;
                manifests.insert(key.as_slice(), data.as_slice())?;
                Ok(())
            })
            .map_err(transaction_error)?;
        self.database.flush_async().await?;
        Ok(id)
    }

    async fn retrieve_parts(&self, ids: &[StreamId]) -> Result<Self::StreamPartStream, Error> {
        // The returned stream owns its IDs and outlives the input slice.
        let ids = ids.to_vec();
        let state = Some((self.clone(), ids.into_iter(), Vec::new().into_iter()));
        Ok(Box::pin(
            stream::unfold(state, |state| async move {
                let (registry, mut ids, mut parts) = state?;
                loop {
                    if let Some(id) = parts.next() {
                        return match registry.get_part(id) {
                            Ok(part) => Some((Ok(part), Some((registry, ids, parts)))),
                            Err(error) => Some((Err(error), None)),
                        };
                    }
                    let id = ids.next()?;
                    match registry.get_manifest(id) {
                        Ok(manifest) => parts = manifest.parts.into_iter(),
                        Err(error) => return Some((Err(error), None)),
                    }
                }
            })
            .fuse(),
        ))
    }
}

use std::{ops::Range, pin::Pin};

use futures_core::Stream;
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use sled::{
    Db, Tree,
    transaction::{
        ConflictableTransactionError, ConflictableTransactionResult, TransactionError,
        Transactional, TransactionalTree,
    },
};

use crate::{
    BlobId, BlobManifest, BlobPart, Error, PartId, StreamId, StreamSlice, traits::BlobRegistry,
};

#[derive(Serialize, Deserialize)]
enum BlobState {
    Open,
    Finalized(BlobManifest),
}

/// A registry with separate sled trees for parts and blob states/manifests.
/// Clones share the database. Successful mutations await its durable flush.
/// Transactions coordinate part insertion and finalization without a mutex.
#[derive(Clone)]
pub struct LocalRegistry {
    database: Db,
    parts: Tree,
    blobs: Tree,
}

impl LocalRegistry {
    /// Uses an existing caller-owned database without opening a stream registry.
    pub fn new(database: Db) -> Result<Self, Error> {
        let parts = database.open_tree("blob-parts")?;
        let blobs = database.open_tree("blob-manifests")?;
        Ok(Self {
            database,
            parts,
            blobs,
        })
    }

    fn get_part(&self, blob_id: BlobId, part_id: PartId) -> Result<BlobPart, Error> {
        let data = self
            .parts
            .get(part_key(blob_id, part_id))?
            .ok_or(Error::PartNotFound { blob_id, part_id })?;
        Ok(postcard::from_bytes(&data)?)
    }

    fn get_manifest(&self, blob_id: BlobId) -> Result<BlobManifest, Error> {
        let data = self
            .blobs
            .get(blob_id.to_be_bytes())?
            .ok_or(Error::BlobNotFound(blob_id))?;
        match postcard::from_bytes(&data)? {
            BlobState::Open => Err(Error::BlobNotFinalized(blob_id)),
            BlobState::Finalized(manifest) => Ok(manifest),
        }
    }
}

fn part_key(blob_id: BlobId, part_id: PartId) -> [u8; 32] {
    let mut key = [0; 32];
    for (target, source) in key.iter_mut().zip(
        blob_id
            .to_be_bytes()
            .into_iter()
            .chain(part_id.to_be_bytes()),
    ) {
        *target = source;
    }
    key
}

fn transaction_error(error: TransactionError<Error>) -> Error {
    match error {
        TransactionError::Abort(error) => error,
        TransactionError::Storage(error) => Error::Database(error),
    }
}

fn check_can_save(
    parts: &TransactionalTree,
    blobs: &TransactionalTree,
    blob_id: BlobId,
    part_id: PartId,
) -> ConflictableTransactionResult<(), Error> {
    let state = blobs
        .get(blob_id.to_be_bytes())?
        .ok_or(ConflictableTransactionError::Abort(Error::BlobNotFound(
            blob_id,
        )))?;
    let state: BlobState = postcard::from_bytes(&state)
        .map_err(|error| ConflictableTransactionError::Abort(Error::Serialization(error)))?;
    if let BlobState::Finalized(_) = state {
        return Err(ConflictableTransactionError::Abort(Error::BlobFinalized(
            blob_id,
        )));
    }
    if parts.get(part_key(blob_id, part_id))?.is_some() {
        return Err(ConflictableTransactionError::Abort(
            Error::PartAlreadyExists { blob_id, part_id },
        ));
    }
    Ok(())
}

impl BlobRegistry for LocalRegistry {
    type StreamSliceStream = Pin<Box<dyn Stream<Item = Result<StreamSlice, Error>> + Send>>;

    async fn create(&self) -> Result<BlobId, Error> {
        let id = BlobId::from(self.database.generate_id()?);
        let key = id.to_be_bytes();
        let data = postcard::to_allocvec(&BlobState::Open)?;
        self.blobs
            .transaction(|blobs| {
                if blobs.get(key)?.is_some() {
                    return Err(ConflictableTransactionError::Abort(
                        Error::BlobAlreadyExists(id),
                    ));
                }
                blobs.insert(key.as_slice(), data.as_slice())?;
                Ok(())
            })
            .map_err(transaction_error)?;
        self.database.flush_async().await?;
        Ok(id)
    }

    async fn can_save(&self, blob_id: BlobId, part_id: PartId) -> Result<(), Error> {
        (&self.parts, &self.blobs)
            .transaction(|(parts, blobs)| check_can_save(parts, blobs, blob_id, part_id))
            .map_err(transaction_error)
    }

    async fn save_part(
        &self,
        blob_id: BlobId,
        stream_id: StreamId,
        length: u64,
        part_id: PartId,
    ) -> Result<(), Error> {
        let key = part_key(blob_id, part_id);
        let data = postcard::to_allocvec(&BlobPart { stream_id, length })?;
        (&self.parts, &self.blobs)
            .transaction(|(parts, blobs)| {
                check_can_save(parts, blobs, blob_id, part_id)?;
                parts.insert(key.as_slice(), data.as_slice())?;
                Ok(())
            })
            .map_err(transaction_error)?;
        self.database.flush_async().await?;
        Ok(())
    }

    async fn finalize(&self, blob_id: BlobId, parts: Vec<PartId>) -> Result<u64, Error> {
        let key = blob_id.to_be_bytes();
        let length = (&self.parts, &self.blobs)
            .transaction(|(part_tree, blobs)| {
                let state = blobs.get(key)?.ok_or(ConflictableTransactionError::Abort(
                    Error::BlobNotFound(blob_id),
                ))?;
                let state: BlobState = postcard::from_bytes(&state).map_err(|error| {
                    ConflictableTransactionError::Abort(Error::Serialization(error))
                })?;
                if let BlobState::Finalized(manifest) = state {
                    return if manifest.parts == parts {
                        Ok(manifest.length)
                    } else {
                        Err(ConflictableTransactionError::Abort(Error::BlobFinalized(
                            blob_id,
                        )))
                    };
                }
                let mut length = 0_u64;
                for &part_id in &parts {
                    let data = part_tree.get(part_key(blob_id, part_id))?.ok_or(
                        ConflictableTransactionError::Abort(Error::PartNotFound {
                            blob_id,
                            part_id,
                        }),
                    )?;
                    let part: BlobPart = postcard::from_bytes(&data).map_err(|error| {
                        ConflictableTransactionError::Abort(Error::Serialization(error))
                    })?;
                    length = length
                        .checked_add(part.length)
                        .ok_or(ConflictableTransactionError::Abort(Error::LengthOverflow))?;
                }
                let data = postcard::to_allocvec(&BlobState::Finalized(BlobManifest {
                    parts: parts.clone(),
                    length,
                }))
                .map_err(|error| {
                    ConflictableTransactionError::Abort(Error::Serialization(error))
                })?;
                blobs.insert(key.as_slice(), data.as_slice())?;
                Ok(length)
            })
            .map_err(transaction_error)?;
        self.database.flush_async().await?;
        Ok(length)
    }

    async fn read(
        &self,
        blob_id: BlobId,
        range: Range<u64>,
    ) -> Result<Self::StreamSliceStream, Error> {
        let manifest = self.get_manifest(blob_id)?;
        if range.start > range.end || range.end > manifest.length {
            return Err(Error::InvalidRange {
                start: range.start,
                end: range.end,
                length: manifest.length,
            });
        }
        if range.start == range.end {
            return Ok(Box::pin(stream::empty()));
        }
        let state = Some((self.clone(), manifest.parts.into_iter(), 0_u64, range));
        Ok(Box::pin(
            stream::unfold(state, move |state| async move {
                let (registry, mut parts, mut offset, range) = state?;
                loop {
                    if offset >= range.end {
                        return None;
                    }
                    let part_id = parts.next()?;
                    let part = match registry.get_part(blob_id, part_id) {
                        Ok(part) => part,
                        Err(error) => return Some((Err(error), None)),
                    };
                    let part_end = match offset.checked_add(part.length) {
                        Some(end) => end,
                        None => return Some((Err(Error::LengthOverflow), None)),
                    };
                    let start = range.start.max(offset);
                    let end = range.end.min(part_end);
                    let skip = start.saturating_sub(offset);
                    offset = part_end;
                    if start < end {
                        let slice = StreamSlice {
                            stream_id: part.stream_id,
                            skip,
                            length: end.saturating_sub(start),
                        };
                        return Some((Ok(slice), Some((registry, parts, offset, range))));
                    }
                }
            })
            .fuse(),
        ))
    }
}

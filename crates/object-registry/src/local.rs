use std::ops::Bound;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sled::{
    Db, Tree,
    transaction::{
        ConflictableTransactionError::Abort, ConflictableTransactionResult,
        Transactional, TransactionalTree,
    },
};

use crate::{
    BlobId, Cursor, DeleteResult, Error, Metadata, PageResult, RecordMetadata, VersionId,
    traits::ObjectRegistry,
};

const DEFAULT_LIMIT: usize = 1_000;

#[derive(Serialize, Deserialize)]
enum VersionContent {
    Object { blob_id: BlobId, metadata: Metadata },
    DeleteMarker,
}

#[derive(Serialize, Deserialize)]
struct VersionEntry<C = VersionContent> {
    previous_version_id: Option<VersionId>,
    deleted: bool,
    content: C,
}

#[derive(Serialize, Deserialize)]
struct ActiveEntry<M = Metadata> {
    version_id: VersionId,
    blob_id: BlobId,
    metadata: M,
}

impl VersionContent {
    fn into_record(self, key: String, version_id: VersionId) -> RecordMetadata {
        let (blob_id, metadata, is_delete_marker) = match self {
            Self::Object { blob_id, metadata } => (Some(blob_id), metadata, false),
            Self::DeleteMarker => (None, Metadata::new(), true),
        };
        RecordMetadata {
            key,
            version_id,
            blob_id,
            metadata,
            is_delete_marker,
        }
    }
}

impl ActiveEntry {
    fn into_record(self, key: String) -> RecordMetadata {
        RecordMetadata {
            key,
            version_id: self.version_id,
            blob_id: Some(self.blob_id),
            metadata: self.metadata,
            is_delete_marker: false,
        }
    }
}

/// Two trees for current objects, version records and per-key current pointers.
/// Transactions coordinate all mutations, including independent handles over
/// the same database. There is no application lock or transactional scan.
#[derive(Clone)]
pub struct LocalRegistry {
    database: Db,
    active: Tree,
    versions: Tree,
}

impl LocalRegistry {
    /// Uses a caller-owned database without opening or accessing a blob registry.
    pub fn new(database: Db) -> Result<Self, Error> {
        let active = database.open_tree("object-active")?;
        let versions = database.open_tree("object-versions")?;
        Ok(Self {
            database,
            active,
            versions,
        })
    }

    async fn flush(&self) -> Result<(), Error> {
        let database = self.database.clone();
        let (sender, receiver) = futures_channel::oneshot::channel();
        // Sled's flush_async shares its pool with log writes and can starve them.
        // ponytail: one thread per flush; use a caller-owned blocking pool if throughput requires it.
        std::thread::Builder::new()
            .name("object-registry-flush".to_owned())
            .spawn(move || {
                let _ = sender.send(database.flush());
            })
            .map_err(Error::FlushWorker)?;
        receiver.await??;
        Ok(())
    }

    fn append(&self, key: &str, content: &VersionContent) -> Result<VersionId, Error> {
        Ok((&self.active, &self.versions)
            .transaction(|(active, versions)| {
                let previous_version_id = current_id(versions, key)?;
                if let Some(previous) = previous_version_id
                    && read_version(versions, key, previous)?.deleted
                {
                    return Err(Abort(Error::CorruptState("current version is deleted")));
                }
                // IDs are generated again on retries, after reading the current pointer.
                let id = versions.generate_id()?;
                check_link(id, previous_version_id)?;
                let version_key = version_key(key, id);
                if versions.get(&version_key)?.is_some() {
                    return Err(Abort(Error::CorruptState(
                        "generated version ID already exists",
                    )));
                }
                // ponytail: borrowed serialization avoids copying maps for transaction retries.
                let entry = VersionEntry {
                    previous_version_id,
                    deleted: false,
                    content,
                };
                versions.insert(version_key, encode(&entry)?)?;
                set_current(active, versions, key, Some((id, content)))?;
                Ok(id)
            })?)
    }
}

fn validate_key(key: &str) -> Result<(), Error> {
    if key.is_empty() || key.as_bytes().contains(&0) {
        return Err(Error::InvalidKey);
    }
    Ok(())
}

fn validate_prefix(prefix: &str) -> Result<(), Error> {
    if prefix.as_bytes().contains(&0) {
        return Err(Error::InvalidPrefix);
    }
    Ok(())
}

fn head_key(key: &str) -> Vec<u8> {
    let mut bytes = vec![0];
    bytes.extend_from_slice(key.as_bytes());
    bytes
}

fn version_prefix(prefix: &str) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(prefix.as_bytes());
    bytes
}

fn version_key(key: &str, id: VersionId) -> Vec<u8> {
    let mut bytes = version_prefix(key);
    bytes.push(0);
    bytes.extend_from_slice(&(!id).to_be_bytes());
    bytes
}

fn parse_object_key(bytes: &[u8]) -> Result<&str, Error> {
    let key = std::str::from_utf8(bytes)?;
    validate_key(key).map_err(|_| Error::CorruptState("invalid stored object key"))?;
    Ok(key)
}

fn parse_version_key(bytes: &[u8]) -> Result<(&str, VersionId), Error> {
    let bytes = bytes
        .strip_prefix(&[1])
        .ok_or(Error::CorruptState("invalid version namespace"))?;
    let separator = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(Error::CorruptState("version key has no separator"))?;
    let key = bytes
        .get(..separator)
        .ok_or(Error::CorruptState("invalid version key boundary"))?;
    let suffix = bytes
        .get(separator..)
        .and_then(|bytes| bytes.strip_prefix(&[0]))
        .ok_or(Error::CorruptState("invalid version key suffix"))?;
    let reverse_id: [u8; 8] = suffix
        .try_into()
        .map_err(|_| Error::CorruptState("version ID must occupy eight bytes"))?;
    Ok((parse_object_key(key)?, !u64::from_be_bytes(reverse_id)))
}

fn check_link(id: VersionId, previous: Option<VersionId>) -> Result<(), Error> {
    if previous.is_some_and(|previous| previous >= id) {
        return Err(Error::CorruptState(
            "previous version ID must be strictly smaller",
        ));
    }
    Ok(())
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    Ok(postcard::to_allocvec(value)?)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    Ok(postcard::from_bytes(bytes)?)
}

fn current_id(
    versions: &TransactionalTree,
    key: &str,
) -> ConflictableTransactionResult<Option<VersionId>, Error> {
    match versions.get(head_key(key))? {
        Some(bytes) => Ok(Some(decode(&bytes)?)),
        None => Ok(None),
    }
}

fn read_version(
    versions: &TransactionalTree,
    key: &str,
    id: VersionId,
) -> ConflictableTransactionResult<VersionEntry, Error> {
    let bytes = versions
        .get(version_key(key, id))?
        .ok_or(Error::CorruptState("referenced version is missing"))?;
    let entry: VersionEntry = decode(&bytes)?;
    check_link(id, entry.previous_version_id)?;
    Ok(entry)
}

fn set_current(
    active: &TransactionalTree,
    versions: &TransactionalTree,
    key: &str,
    current: Option<(VersionId, &VersionContent)>,
) -> ConflictableTransactionResult<(), Error> {
    match current {
        Some((id, content)) => {
            versions.insert(head_key(key), encode(&id)?)?;
            match content {
                VersionContent::Object { blob_id, metadata } => {
                    let entry = ActiveEntry {
                        version_id: id,
                        blob_id: *blob_id,
                        metadata,
                    };
                    active.insert(key.as_bytes(), encode(&entry)?)?;
                }
                VersionContent::DeleteMarker => {
                    active.remove(key.as_bytes())?;
                }
            }
        }
        None => {
            versions.remove(head_key(key))?;
            active.remove(key.as_bytes())?;
        }
    }
    Ok(())
}

fn validate_cursor(cursor: &Cursor, prefix: &str, include_versions: bool) -> Result<(), Error> {
    if cursor.prefix != prefix || cursor.include_versions != include_versions {
        return Err(Error::InvalidCursor);
    }
    let object_key = if include_versions {
        parse_version_key(&cursor.last_key)
            .map_err(|_| Error::InvalidCursor)?
            .0
    } else {
        parse_object_key(&cursor.last_key).map_err(|_| Error::InvalidCursor)?
    };
    if !object_key.starts_with(prefix) {
        return Err(Error::InvalidCursor);
    }
    Ok(())
}

impl ObjectRegistry for LocalRegistry {
    async fn store(
        &self,
        key: &str,
        blob_id: BlobId,
        metadata: Metadata,
    ) -> Result<VersionId, Error> {
        validate_key(key)?;
        let id = self.append(key, &VersionContent::Object { blob_id, metadata })?;
        self.flush().await?;
        Ok(id)
    }

    async fn head(
        &self,
        key: &str,
        version_id: Option<VersionId>,
    ) -> Result<RecordMetadata, Error> {
        validate_key(key)?;
        match version_id {
            Some(id) => {
                let bytes = self.versions.get(version_key(key, id))?.ok_or_else(|| {
                    Error::VersionNotFound {
                        key: key.to_owned(),
                        version_id: id,
                    }
                })?;
                let entry: VersionEntry = decode(&bytes)?;
                check_link(id, entry.previous_version_id)?;
                if entry.deleted {
                    return Err(Error::VersionNotFound {
                        key: key.to_owned(),
                        version_id: id,
                    });
                }
                Ok(entry.content.into_record(key.to_owned(), id))
            }
            None => {
                let bytes = self
                    .active
                    .get(key.as_bytes())?
                    .ok_or_else(|| Error::ObjectNotFound(key.to_owned()))?;
                let entry: ActiveEntry = decode(&bytes)?;
                Ok(entry.into_record(key.to_owned()))
            }
        }
    }

    async fn list(
        &self,
        prefix: &str,
        cursor: Option<&Cursor>,
        limit: Option<usize>,
        include_versions: bool,
    ) -> Result<PageResult, Error> {
        validate_prefix(prefix)?;
        let limit = limit.unwrap_or(DEFAULT_LIMIT);
        if limit == 0 {
            return Err(Error::InvalidLimit);
        }
        if let Some(cursor) = cursor {
            validate_cursor(cursor, prefix, include_versions)?;
        }
        let (tree, scan_prefix) = if include_versions {
            (&self.versions, version_prefix(prefix))
        } else {
            (&self.active, prefix.as_bytes().to_vec())
        };
        let lower = match cursor {
            Some(cursor) => Bound::Excluded(cursor.last_key.clone()),
            None => Bound::Included(scan_prefix.clone()),
        };
        let mut records = Vec::new();
        let mut last_key = None;
        for item in tree.range((lower, Bound::<Vec<u8>>::Unbounded)) {
            let (stored_key, bytes) = item?;
            if !stored_key.starts_with(&scan_prefix) {
                break;
            }
            let record = if include_versions {
                let (key, id) = parse_version_key(&stored_key)?;
                let entry: VersionEntry = decode(&bytes)?;
                check_link(id, entry.previous_version_id)?;
                if entry.deleted {
                    continue;
                }
                entry.content.into_record(key.to_owned(), id)
            } else {
                let key = parse_object_key(&stored_key)?;
                let entry: ActiveEntry = decode(&bytes)?;
                entry.into_record(key.to_owned())
            };
            if records.len() == limit {
                let last_key =
                    last_key.ok_or(Error::CorruptState("listing has no continuation bound"))?;
                return Ok(PageResult {
                    records,
                    next_cursor: Some(Cursor {
                        prefix: prefix.to_owned(),
                        include_versions,
                        last_key,
                    }),
                });
            }
            records.push(record);
            last_key = Some(stored_key.to_vec());
        }
        Ok(PageResult {
            records,
            next_cursor: None,
        })
    }

    async fn delete(
        &self,
        key: &str,
        version_id: Option<VersionId>,
    ) -> Result<DeleteResult, Error> {
        validate_key(key)?;
        let version_id = match version_id {
            None => self.append(key, &VersionContent::DeleteMarker)?,
            Some(id) => {
                (&self.active, &self.versions)
                    .transaction(|(active, versions)| {
                        let stored_key = version_key(key, id);
                        let Some(bytes) = versions.get(&stored_key)? else {
                            return Ok(());
                        };
                        let mut entry: VersionEntry = decode(&bytes)?;
                        check_link(id, entry.previous_version_id)?;
                        if entry.deleted {
                            return Ok(());
                        }
                        if current_id(versions, key)? == Some(id) {
                            let mut previous = entry.previous_version_id;
                            let mut replacement = None;
                            while let Some(previous_id) = previous {
                                let candidate = read_version(versions, key, previous_id)?;
                                if !candidate.deleted {
                                    replacement = Some((previous_id, candidate));
                                    break;
                                }
                                previous = candidate.previous_version_id;
                            }
                            set_current(
                                active,
                                versions,
                                key,
                                replacement
                                    .as_ref()
                                    .map(|(id, entry)| (*id, &entry.content)),
                            )?;
                        }
                        entry.deleted = true;
                        versions.insert(stored_key, encode(&entry)?)?;
                        Ok(())
                    })?;
                id
            }
        };
        self.flush().await?;
        Ok(DeleteResult { version_id })
    }
}

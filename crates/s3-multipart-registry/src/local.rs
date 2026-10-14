use std::collections::BTreeMap;

use sled::{Db, Tree, transaction::Transactional};

use crate::{
    BlobId, Cursor, Error, MultipartUpload, MultipartUploadId, PageResult, UploadRecord,
    traits::{S3MultipartRegistry, validate_listing},
};

/// Clones and independent handles over the same database share atomic mappings.
#[derive(Clone)]
pub struct LocalRegistry {
    database: Db,
    uploads: Tree,
    blobs: Tree,
}

impl LocalRegistry {
    pub fn new(database: Db) -> Result<Self, Error> {
        Ok(Self {
            uploads: database.open_tree("s3-multipart-uploads")?,
            blobs: database.open_tree("s3-multipart-blobs")?,
            database,
        })
    }

    async fn flush(&self) -> Result<(), Error> {
        let database = self.database.clone();
        let (sender, receiver) = futures_channel::oneshot::channel();
        // ponytail: one thread per flush, matching the other registries to avoid sled I/O pool starvation.
        std::thread::Builder::new()
            .name("s3-multipart-registry-flush".to_owned())
            .spawn(move || {
                let _ = sender.send(database.flush());
            })
            .map_err(Error::FlushWorker)?;
        receiver.await??;
        Ok(())
    }

    // A collision changes neither tree; the caller generates another candidate.
    fn insert(&self, id: &MultipartUploadId, blob_id: BlobId, data: &[u8]) -> Result<bool, Error> {
        Ok(
            (&self.uploads, &self.blobs).transaction(|(uploads, blobs)| {
                let blob_key = blob_id.to_be_bytes();
                if blobs.get(blob_key)?.is_some() {
                    return Err(Error::BlobAlreadyExists(blob_id).into());
                }
                if uploads.get(id.as_str().as_bytes())?.is_some() {
                    return Ok(false);
                }
                uploads.insert(id.as_str().as_bytes(), data)?;
                blobs.insert(blob_key.as_slice(), id.as_str().as_bytes())?;
                Ok(true)
            })?,
        )
    }
}

fn decode_upload(data: &[u8]) -> Result<MultipartUpload, Error> {
    let (upload, tail) = postcard::take_from_bytes(data)?;
    if !tail.is_empty() {
        return Err(Error::CorruptState("trailing serialized bytes"));
    }
    Ok(upload)
}

impl S3MultipartRegistry for LocalRegistry {
    async fn create(&self, upload: MultipartUpload) -> Result<MultipartUploadId, Error> {
        let data = postcard::to_allocvec(&upload)?;
        loop {
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random)?;
            let id = MultipartUploadId::new(format!("{:032x}", u128::from_be_bytes(random)))?;
            if self.insert(&id, upload.blob_id, &data)? {
                self.flush().await?;
                return Ok(id);
            }
        }
    }

    async fn get(&self, id: &MultipartUploadId) -> Result<Option<MultipartUpload>, Error> {
        self.uploads
            .get(id.as_str().as_bytes())?
            .map(|data| decode_upload(&data))
            .transpose()
    }

    async fn list(
        &self,
        bucket: &str,
        prefix: &str,
        cursor: Option<&Cursor>,
        limit: Option<usize>,
    ) -> Result<PageResult, Error> {
        let limit = validate_listing(bucket, prefix, cursor, limit)?;
        let retained_limit = limit.checked_add(1).ok_or(Error::InvalidLimit)?;
        let mut selected = BTreeMap::new();

        // Sessions are indexed by random ID. Scan them all but retain only the
        // first page and one lookahead entry in key/ID order. No index is added.
        for entry in self.uploads.iter() {
            let (stored_id, data) = entry?;
            let upload = decode_upload(&data)?;
            if upload.bucket != bucket || !upload.key.starts_with(prefix) {
                continue;
            }
            let id = std::str::from_utf8(&stored_id)
                .map_err(|_| Error::CorruptState("invalid stored upload ID"))?
                .parse::<MultipartUploadId>()
                .map_err(|_| Error::CorruptState("invalid stored upload ID"))?;
            if cursor.is_some_and(|cursor| {
                upload.key < cursor.key
                    || (upload.key == cursor.key
                        && cursor.upload_id.as_ref().is_none_or(|after| id <= *after))
            }) {
                continue;
            }
            selected.insert((upload.key.clone(), id), upload);
            if selected.len() > retained_limit {
                let _ = selected.pop_last();
            }
        }

        let has_more = selected.len() > limit;
        if has_more {
            let _ = selected.pop_last();
        }
        let records: Vec<_> = selected
            .into_iter()
            .map(|((_, upload_id), upload)| UploadRecord { upload_id, upload })
            .collect();
        let next_cursor = if has_more {
            let last = records
                .last()
                .ok_or(Error::CorruptState("empty listing page"))?;
            Some(Cursor {
                bucket: bucket.to_owned(),
                prefix: prefix.to_owned(),
                key: last.upload.key.clone(),
                upload_id: Some(last.upload_id.clone()),
            })
        } else {
            None
        };
        Ok(PageResult {
            records,
            next_cursor,
        })
    }

    async fn remove(&self, id: &MultipartUploadId) -> Result<(), Error> {
        (&self.uploads, &self.blobs).transaction(|(uploads, blobs)| {
            let Some(data) = uploads.get(id.as_str().as_bytes())? else {
                return Ok(());
            };
            let upload = decode_upload(&data)?;
            let blob_key = upload.blob_id.to_be_bytes();
            let reverse = blobs
                .get(blob_key)?
                .ok_or(Error::CorruptState("missing reverse mapping"))?;
            if reverse.as_ref() != id.as_str().as_bytes() {
                return Err(Error::CorruptState("inconsistent reverse mapping").into());
            }
            uploads.remove(id.as_str().as_bytes())?;
            blobs.remove(blob_key.as_slice())?;
            Ok(())
        })?;
        self.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_resumes_by_key_and_id_after_removal() -> Result<(), Box<dyn std::error::Error>> {
        futures_executor::block_on(async {
            let database = sled::Config::new().temporary(true).open()?;
            let registry = LocalRegistry::new(database)?;
            for (blob_id, bucket, key, id) in [
                (1_u128, "bucket", "p/b", 1_u128),
                (2, "bucket", "p/a", 3),
                (3, "bucket", "p/a", 2),
                (4, "other", "p/a", 4),
                (5, "bucket", "else", 5),
            ] {
                let id = MultipartUploadId::new(format!("{id:032x}"))?;
                let upload = MultipartUpload {
                    blob_id,
                    bucket: bucket.to_owned(),
                    key: key.to_owned(),
                    metadata: BTreeMap::from([("custom".to_owned(), "value".to_owned())]),
                };
                if !registry.insert(&id, blob_id, &postcard::to_allocvec(&upload)?)? {
                    return Err("fixture ID collided".into());
                }
            }
            let first = registry.list("bucket", "p/", None, Some(1)).await?;
            let record = first.records.first().ok_or("missing first record")?;
            if first.records.len() != 1
                || record.upload.key != "p/a"
                || record.upload_id.as_str() != format!("{:032x}", 2_u128)
                || record.upload.metadata.get("custom").map(String::as_str) != Some("value")
            {
                return Err("listing changed order, scope or metadata".into());
            }
            let cursor = first.next_cursor.ok_or("missing first cursor")?;
            let cursor: Cursor = postcard::from_bytes(&postcard::to_allocvec(&cursor)?)?;
            registry
                .remove(
                    cursor
                        .upload_id
                        .as_ref()
                        .ok_or("missing cursor upload ID")?,
                )
                .await?;
            let second = registry
                .list("bucket", "p/", Some(&cursor), Some(1))
                .await?;
            let record = second.records.first().ok_or("missing second record")?;
            if second.records.len() != 1
                || record.upload.key != "p/a"
                || record.upload_id.as_str() != format!("{:032x}", 3_u128)
            {
                return Err("removed marker skipped another upload for its key".into());
            }
            let cursor = second.next_cursor.ok_or("missing second cursor")?;
            let last = registry
                .list("bucket", "p/", Some(&cursor), Some(1))
                .await?;
            if last.records.len() != 1
                || last
                    .records
                    .first()
                    .map(|record| record.upload.key.as_str())
                    != Some("p/b")
                || last.next_cursor.is_some()
            {
                return Err("full final page was truncated".into());
            }
            let key_only = Cursor {
                upload_id: None,
                ..cursor
            };
            let after_key = registry
                .list("bucket", "p/", Some(&key_only), Some(1))
                .await?;
            if after_key != last {
                return Err("key-only cursor did not skip every upload for the key".into());
            }
            if !matches!(
                registry
                    .list("bucket", "other/", Some(&key_only), Some(1))
                    .await,
                Err(Error::InvalidCursor)
            ) {
                return Err("cursor accepted a different prefix".into());
            }
            for limit in [0, 1_001] {
                if !matches!(
                    registry.list("bucket", "", None, Some(limit)).await,
                    Err(Error::InvalidLimit)
                ) {
                    return Err("invalid page limit accepted".into());
                }
            }
            let empty = registry.list("absent", "", None, None).await?;
            if !empty.records.is_empty() || empty.next_cursor.is_some() {
                return Err("empty bucket returned a page".into());
            }
            Ok(())
        })
    }

    #[test]
    fn collision_preserves_both_mappings() -> Result<(), Box<dyn std::error::Error>> {
        let database = sled::Config::new().temporary(true).open()?;
        let registry = LocalRegistry::new(database)?;
        let id = MultipartUploadId::new("a".repeat(32))?;
        let first = MultipartUpload {
            blob_id: 1,
            bucket: String::new(),
            key: String::new(),
            metadata: Default::default(),
        };
        let second = MultipartUpload {
            blob_id: 2,
            ..first.clone()
        };
        let first_data = postcard::to_allocvec(&first)?;
        if !registry.insert(&id, 1, &first_data)? {
            return Err("first insertion collided".into());
        }
        if registry.insert(&id, 2, &postcard::to_allocvec(&second)?)?
            || registry.blobs.get(2_u128.to_be_bytes())?.is_some()
            || registry.uploads.get(id.as_str().as_bytes())?.as_deref()
                != Some(first_data.as_slice())
        {
            return Err("collision changed registry state".into());
        }
        Ok(())
    }
}

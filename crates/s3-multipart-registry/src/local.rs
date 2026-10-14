use sled::{Db, Tree, transaction::Transactional};

use crate::{BlobId, Error, MultipartUpload, MultipartUploadId, traits::S3MultipartRegistry};

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

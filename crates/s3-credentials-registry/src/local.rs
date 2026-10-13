use sled::{Db, Tree};

use crate::{Credential, Error, KeyId, KeySecret, PrincipalId, traits::S3CredentialRegistry};

/// Clones and independent handles over the same database share atomic state.
#[derive(Clone)]
pub struct LocalRegistry {
    database: Db,
    credentials: Tree,
}

impl LocalRegistry {
    pub fn new(database: Db) -> Result<Self, Error> {
        Ok(Self {
            credentials: database.open_tree("s3-credentials")?,
            database,
        })
    }

    async fn flush(&self) -> Result<(), Error> {
        let database = self.database.clone();
        let (sender, receiver) = futures_channel::oneshot::channel();
        // ponytail: one thread per flush, matching the other registries to avoid sled I/O pool starvation.
        std::thread::Builder::new()
            .name("s3-credentials-registry-flush".to_owned())
            .spawn(move || {
                let _ = sender.send(database.flush());
            })
            .map_err(Error::FlushWorker)?;
        receiver.await??;
        Ok(())
    }
}

fn decode_credential(data: &[u8]) -> Result<Credential, Error> {
    let (credential, tail) = postcard::take_from_bytes(data)?;
    if !tail.is_empty() {
        return Err(Error::CorruptState("trailing serialized bytes"));
    }
    Ok(credential)
}

impl S3CredentialRegistry for LocalRegistry {
    async fn register_key(
        &self,
        id: KeyId,
        secret: KeySecret,
        principal: PrincipalId,
    ) -> Result<(), Error> {
        let data = postcard::to_allocvec(&Credential {
            principal_id: principal,
            secret,
        })?;
        if self
            .credentials
            .compare_and_swap(id.as_str().as_bytes(), None::<&[u8]>, Some(data.as_slice()))?
            .is_err()
        {
            return Err(Error::KeyAlreadyExists(id));
        }
        self.flush().await
    }

    async fn get_key(&self, id: &KeyId) -> Result<Option<Credential>, Error> {
        let Some(data) = self.credentials.get(id.as_str().as_bytes())? else {
            return Ok(None);
        };
        Ok(Some(decode_credential(&data)?))
    }

    async fn list_keys(&self, principal: PrincipalId) -> Result<Vec<KeyId>, Error> {
        let mut keys = Vec::new();
        // ponytail: O(total keys) scan; add a principal index if listing throughput requires it.
        for entry in self.credentials.iter() {
            let (id, data) = entry?;
            if decode_credential(&data)?.principal_id == principal {
                let id = std::str::from_utf8(&id)
                    .map_err(|_| Error::CorruptState("invalid key ID encoding"))?;
                keys.push(
                    KeyId::new(id.to_owned())
                        .map_err(|_| Error::CorruptState("invalid key ID length"))?,
                );
            }
        }
        Ok(keys)
    }

    async fn revoke_key(&self, id: &KeyId) -> Result<(), Error> {
        self.credentials.remove(id.as_str().as_bytes())?;
        self.flush().await
    }
}

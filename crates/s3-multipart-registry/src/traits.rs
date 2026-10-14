use std::{collections::BTreeMap, future::Future, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{BlobId, Error};

/// An opaque identifier containing exactly 32 lowercase hexadecimal ASCII characters.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct MultipartUploadId(String);

impl MultipartUploadId {
    pub fn new(value: String) -> Result<Self, Error> {
        if value.len() != 32
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::InvalidUploadId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for MultipartUploadId {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        Self::new(value)
    }
}

impl FromStr for MultipartUploadId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Error> {
        Self::new(value.to_owned())
    }
}

/// Immutable session metadata. Blob existence and S3 authorization belong to the caller.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MultipartUpload {
    pub blob_id: BlobId,
    pub bucket: String,
    pub key: String,
    pub metadata: BTreeMap<String, String>,
}

pub trait S3MultipartRegistry {
    /// Generates an unpredictable ID and durably inserts a session.
    /// Rejects a blob already associated with another session.
    fn create(
        &self,
        upload: MultipartUpload,
    ) -> impl Future<Output = Result<MultipartUploadId, Error>> + Send;

    fn get(
        &self,
        id: &MultipartUploadId,
    ) -> impl Future<Output = Result<Option<MultipartUpload>, Error>> + Send;

    /// Durably removes the session and reverse mapping, without touching the blob.
    /// Removing an absent session succeeds.
    fn remove(&self, id: &MultipartUploadId) -> impl Future<Output = Result<(), Error>> + Send;
}

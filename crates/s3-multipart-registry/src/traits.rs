use std::{collections::BTreeMap, future::Future, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{BlobId, Error};

/// An opaque identifier containing exactly 32 lowercase hexadecimal ASCII characters.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
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

/// An exclusive listing bound scoped to a bucket and prefix.
/// Without an upload ID, all sessions for the marker key are skipped.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    pub bucket: String,
    pub prefix: String,
    pub key: String,
    pub upload_id: Option<MultipartUploadId>,
}

/// An owned session and its upload identifier.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UploadRecord {
    pub upload_id: MultipartUploadId,
    pub upload: MultipartUpload,
}

/// A bounded page sorted by key, then upload ID. Pages are not snapshots.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PageResult {
    pub records: Vec<UploadRecord>,
    pub next_cursor: Option<Cursor>,
}

pub(crate) fn validate_listing(
    bucket: &str,
    prefix: &str,
    cursor: Option<&Cursor>,
    limit: Option<usize>,
) -> Result<usize, Error> {
    if bucket.is_empty() || bucket.contains(['/', '\0']) {
        return Err(Error::InvalidBucket);
    }
    if prefix.contains('\0') {
        return Err(Error::InvalidPrefix);
    }
    if cursor.is_some_and(|cursor| {
        cursor.bucket != bucket || cursor.prefix != prefix || cursor.key.contains('\0')
    }) {
        return Err(Error::InvalidCursor);
    }
    let limit = limit.unwrap_or(1_000);
    if !(1..=1_000).contains(&limit) {
        return Err(Error::InvalidLimit);
    }
    Ok(limit)
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

    /// Lists sessions in one bucket whose keys start with prefix.
    /// The cursor is an exclusive key/ID bound, or a key-only bound, and must
    /// match the bucket and prefix. Limits range from 1 to 1,000, default 1,000.
    /// Results own their data and provide no snapshot or reservation guarantee.
    fn list(
        &self,
        bucket: &str,
        prefix: &str,
        cursor: Option<&Cursor>,
        limit: Option<usize>,
    ) -> impl Future<Output = Result<PageResult, Error>> + Send;

    /// Durably removes the session and reverse mapping, without touching the blob.
    /// Removing an absent session succeeds.
    fn remove(&self, id: &MultipartUploadId) -> impl Future<Output = Result<(), Error>> + Send;
}

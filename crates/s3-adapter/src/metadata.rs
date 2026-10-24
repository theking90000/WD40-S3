//! Interprets the object metadata conventions used by the adapters.

use std::time::SystemTime;

use object_registry::{Metadata, VersionId};
use s3s::{
    S3Error, S3Result,
    dto::{ETag, Timestamp, TimestampFormat},
};

const STORED_CREDENTIAL_KEY: &str = "wd40-credential-id";
pub(crate) const UNAUTHENTICATED_CREDENTIAL: &str = "unauthenticated";

pub(crate) fn is_reserved_key(key: &str) -> bool {
    [
        "size",
        "content-type",
        "last-modified",
        STORED_CREDENTIAL_KEY,
    ]
    .iter()
    .any(|reserved| key.eq_ignore_ascii_case(reserved))
}

/// Persists attribution even when metrics collection is disabled.
pub(crate) fn set_stored_credential(metadata: &mut Metadata, credential: Option<&str>) {
    strip_stored_credential(metadata);
    metadata.insert(
        STORED_CREDENTIAL_KEY.to_owned(),
        credential.unwrap_or(UNAUTHENTICATED_CREDENTIAL).to_owned(),
    );
}

/// None denotes a legacy unknown attribution, not an anonymous publisher.
#[cfg(feature = "metrics")]
pub(crate) fn stored_credential(metadata: &Metadata) -> Option<&str> {
    metadata.get(STORED_CREDENTIAL_KEY).map(String::as_str)
}

pub(crate) fn strip_stored_credential(metadata: &mut Metadata) {
    metadata.retain(|key, _| !key.eq_ignore_ascii_case(STORED_CREDENTIAL_KEY));
}

pub(crate) fn size(metadata: &Metadata) -> S3Result<u64> {
    metadata
        .get("size")
        .ok_or_else(|| s3s::s3_error!(InternalError, "missing object size metadata"))?
        .parse::<u64>()
        .map_err(S3Error::internal_error)
}

pub(crate) fn set_last_modified(metadata: &mut Metadata) -> S3Result<Timestamp> {
    let timestamp = Timestamp::from(SystemTime::now());
    let mut value = Vec::new();
    timestamp
        .format(TimestampFormat::DateTime, &mut value)
        .map_err(S3Error::internal_error)?;
    let value = String::from_utf8(value).map_err(S3Error::internal_error)?;
    // Return the persisted millisecond precision, including in CopyObject.
    let timestamp =
        Timestamp::parse(TimestampFormat::DateTime, &value).map_err(S3Error::internal_error)?;
    metadata.insert("last-modified".to_owned(), value);
    Ok(timestamp)
}

pub(crate) fn take_last_modified(metadata: &mut Metadata) -> S3Result<Timestamp> {
    match metadata.remove("last-modified") {
        Some(value) => {
            Timestamp::parse(TimestampFormat::DateTime, &value).map_err(S3Error::internal_error)
        }
        // Legacy objects use September 10, 2025 at 12:23 UTC-6, or 18:23 UTC.
        None => Timestamp::parse(TimestampFormat::DateTime, "2025-09-10T18:23:00.000Z")
            .map_err(S3Error::internal_error),
    }
}

pub(crate) fn take_size(metadata: &mut Metadata) -> S3Result<u64> {
    let value = size(metadata)?;
    metadata.remove("size");
    Ok(value)
}

pub(crate) fn content_length(size: u64) -> S3Result<i64> {
    i64::try_from(size).map_err(S3Error::internal_error)
}

pub(crate) fn take_content_type(metadata: &mut Metadata) -> S3Result<String> {
    metadata
        .remove("content-type")
        .ok_or_else(|| s3s::s3_error!(InternalError, "missing object content-type metadata"))
}

pub(crate) fn etag_from_version(version_id: VersionId) -> ETag {
    ETag::Strong(version_id.to_string())
}

//! Interprets the object metadata conventions used by the adapters.

use std::time::SystemTime;

use object_registry::{Metadata, VersionId};
use s3s::{
    S3Error, S3Result,
    dto::{ETag, Timestamp, TimestampFormat},
};

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
    metadata
        .remove("size")
        .ok_or_else(|| s3s::s3_error!(InternalError, "missing object size metadata"))?
        .parse::<u64>()
        .map_err(S3Error::internal_error)
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

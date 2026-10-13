//! Interprets the object metadata conventions used by the adapters.

use object_registry::{Metadata, VersionId};
use s3s::{S3Error, S3Result, dto::ETag};

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

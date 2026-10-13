//! Converts registry errors to S3 errors while preserving their causes.

use s3s::S3Error;

pub(crate) fn object_registry_error(error: object_registry::Error) -> S3Error {
    match error {
        error @ object_registry::Error::ObjectNotFound(_) => {
            s3s::s3_error!(error, NoSuchKey)
        }
        error @ object_registry::Error::VersionNotFound { .. } => {
            s3s::s3_error!(error, NoSuchVersion)
        }
        error @ (object_registry::Error::InvalidKey
        | object_registry::Error::InvalidPrefix
        | object_registry::Error::InvalidLimit
        | object_registry::Error::InvalidCursor) => {
            s3s::s3_error!(error, InvalidArgument)
        }
        error => S3Error::internal_error(error),
    }
}

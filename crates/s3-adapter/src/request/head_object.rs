use s3s::{
    S3Request, S3Response, S3Result,
    dto::{ETag, HeadObjectInput, HeadObjectOutput},
};

use access_controller::Action;
use object_registry::traits::ObjectRegistry;

use crate::{S3Adapter, access::S3Access, to_key};

pub(crate) async fn head_object_access(
    access: &S3Access,
    req: &mut S3Request<HeadObjectInput>,
) -> S3Result<()> {
    access
        .authorize(req.credentials.as_ref(), &to_key!(req.input), Action::Read)
        .await
}

pub(crate) async fn head_object(
    adapter: &S3Adapter,
    req: S3Request<HeadObjectInput>,
) -> S3Result<S3Response<HeadObjectOutput>> {
    let record = adapter
        .objects
        .head(&to_key!(req.input), None)
        .await
        .map_err(|error| match error {
            error @ object_registry::Error::ObjectNotFound(_) => {
                s3s::s3_error!(error, NoSuchKey)
            }
            error @ object_registry::Error::VersionNotFound { .. } => {
                s3s::s3_error!(error, NoSuchVersion)
            }
            error @ object_registry::Error::InvalidKey => {
                s3s::s3_error!(error, InvalidArgument)
            }
            error => s3s::S3Error::internal_error(error),
        })?;

    if record.is_delete_marker {
        return Err(s3s::s3_error!(NoSuchKey));
    }

    let mut metadata = record.metadata;

    let length = metadata
        .remove("size")
        .ok_or_else(|| s3s::s3_error!(InternalError, "missing object size metadata"))?
        .parse::<u64>()
        .map_err(s3s::S3Error::internal_error)?;

    let content_length = i64::try_from(length).map_err(s3s::S3Error::internal_error)?;

    let content_type = metadata
        .remove("content-type")
        .ok_or_else(|| s3s::s3_error!(InternalError, "missing object content-type metadata"))?;

    let version_id = record.version_id.to_string();

    Ok(S3Response::new(HeadObjectOutput {
        accept_ranges: Some("bytes".to_owned()),
        content_length: Some(content_length),
        content_type: Some(content_type),
        e_tag: Some(ETag::Strong(version_id.clone())),
        metadata: Some(metadata.into_iter().collect()),
        version_id: Some(version_id),
        ..Default::default()
    }))
}

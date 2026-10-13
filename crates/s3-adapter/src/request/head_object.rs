use s3s::{
    S3Request, S3Response, S3Result,
    dto::{HeadObjectInput, HeadObjectOutput},
};

use access_controller::Action;
use object_registry::traits::ObjectRegistry;

use crate::{S3Adapter, access::S3Access, error, metadata, to_key};

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
        .map_err(error::object_registry_error)?;

    if record.is_delete_marker {
        return Err(s3s::s3_error!(NoSuchKey));
    }

    let mut metadata = record.metadata;

    let length = metadata::take_size(&mut metadata)?;
    let content_length = metadata::content_length(length)?;
    let content_type = metadata::take_content_type(&mut metadata)?;

    let version_id = record.version_id.to_string();

    Ok(S3Response::new(HeadObjectOutput {
        accept_ranges: Some("bytes".to_owned()),
        content_length: Some(content_length),
        content_type: Some(content_type),
        e_tag: Some(metadata::etag_from_version(record.version_id)),
        metadata: Some(metadata.into_iter().collect()),
        version_id: Some(version_id),
        ..Default::default()
    }))
}

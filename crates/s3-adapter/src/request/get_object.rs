use std::ops::Range;

use access_controller::Action;
use object_registry::traits::ObjectRegistry;
use s3s::{
    S3Request, S3Response, S3Result,
    dto::{GetObjectInput, GetObjectOutput},
};

use crate::{S3Adapter, access::S3Access, body, error, metadata, to_key};

pub(crate) async fn get_object_access(
    access: &S3Access,
    req: &mut S3Request<GetObjectInput>,
) -> S3Result<()> {
    access
        .authorize(req.credentials.as_ref(), &to_key!(req.input), Action::Read)
        .await
}

pub(crate) async fn get_object(
    adapter: &S3Adapter,
    req: S3Request<GetObjectInput>,
) -> S3Result<S3Response<GetObjectOutput>> {
    let record = adapter
        .objects
        .head(&to_key!(req.input), None)
        .await
        .map_err(error::object_registry_error)?;

    if record.is_delete_marker {
        return Err(s3s::s3_error!(NoSuchKey));
    }
    let blob_id = record
        .blob_id
        .ok_or_else(|| s3s::s3_error!(InternalError, "object has no blob reference"))?;

    let mut metadata = record.metadata;
    let length = metadata::take_size(&mut metadata)?;
    let content_type = metadata::take_content_type(&mut metadata)?;
    let last_modified = metadata::take_last_modified(&mut metadata)?;

    let (selected, content_length, content_range) = select_range(req.input.range, length)?;
    // Read the same exclusive-end selection used to build the response headers.
    // The body module contains the temporary task/channel bridge needed to keep
    // CARBON progressing during HTTP backpressure. TODO(s3-body-progress): its
    // documented replacement must preserve this request-level range contract.
    let body = body::read_blob(adapter, blob_id, selected).await?;

    Ok(S3Response::new(GetObjectOutput {
        accept_ranges: Some("bytes".to_owned()),
        content_length: Some(content_length),
        content_range,
        content_type: Some(content_type),
        e_tag: Some(metadata::etag_from_version(record.version_id)),
        last_modified: Some(last_modified),
        metadata: Some(metadata.into_iter().collect()),
        version_id: Some(record.version_id.to_string()),
        body: Some(body),
        ..Default::default()
    }))
}

fn select_range(
    requested: Option<s3s::dto::Range>,
    length: u64,
) -> S3Result<(Range<u64>, i64, Option<String>)> {
    match requested {
        None => Ok((0..length, metadata::content_length(length)?, None)),
        Some(range) => {
            let selected = range
                .check(length)
                .map_err(|error| s3s::s3_error!(error, InvalidRange))?;
            if selected.is_empty() {
                return Err(s3s::s3_error!(InvalidRange));
            }
            let selected_length = selected
                .end
                .checked_sub(selected.start)
                .ok_or_else(|| s3s::s3_error!(InvalidRange))?;
            let last = selected
                .end
                .checked_sub(1)
                .ok_or_else(|| s3s::s3_error!(InvalidRange))?;
            let content_length = metadata::content_length(selected_length)?;
            let content_range = format!("bytes {}-{last}/{length}", selected.start);
            Ok((selected, content_length, Some(content_range)))
        }
    }
}

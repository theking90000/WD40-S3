use access_controller::Action;
use s3_multipart_registry::{MultipartUploadId, traits::S3MultipartRegistry as _};
use s3s::{
    S3Error, S3Request, S3Response, S3Result,
    dto::{AbortMultipartUploadInput, AbortMultipartUploadOutput},
};

use crate::{S3Adapter, access::S3Access, to_key};

pub(crate) async fn abort_multipart_upload_access(
    access: &S3Access,
    req: &mut S3Request<AbortMultipartUploadInput>,
) -> S3Result<()> {
    access
        .authorize(
            req.credentials.as_ref(),
            &to_key!(req.input),
            Action::Delete,
        )
        .await
}

pub(crate) async fn abort_multipart_upload(
    adapter: &S3Adapter,
    req: S3Request<AbortMultipartUploadInput>,
) -> S3Result<S3Response<AbortMultipartUploadOutput>> {
    let input = req.input;
    validate_input(&input)?;

    let upload_id = input
        .upload_id
        .parse::<MultipartUploadId>()
        .map_err(|_| s3s::s3_error!(NoSuchUpload))?;
    let upload = adapter
        .multipart
        .get(&upload_id)
        .await
        .map_err(S3Error::internal_error)?
        .ok_or_else(|| s3s::s3_error!(NoSuchUpload))?;
    if upload.bucket != input.bucket || upload.key != input.key {
        return Err(s3s::s3_error!(NoSuchUpload));
    }

    // Removing the session leaves the blob and any in-flight operations untouched.
    adapter
        .multipart
        .remove(&upload_id)
        .await
        .map_err(S3Error::internal_error)?;

    Ok(S3Response::new(AbortMultipartUploadOutput::default()))
}

fn validate_input(input: &AbortMultipartUploadInput) -> S3Result<()> {
    if input.bucket.is_empty()
        || input.bucket.contains(['/', '\0'])
        || input.key.is_empty()
        || input.key.contains('\0')
    {
        return Err(s3s::s3_error!(InvalidArgument, "invalid bucket or key"));
    }

    reject_unsupported!(
        "AbortMultipartUpload",
        input,
        expected_bucket_owner,
        request_payer,
        if_match_initiated_time,
    );

    Ok(())
}

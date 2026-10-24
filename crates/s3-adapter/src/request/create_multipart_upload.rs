use access_controller::Action;
use s3_multipart_registry::{MultipartUpload, traits::S3MultipartRegistry as _};
use s3s::{
    S3Error, S3Request, S3Response, S3Result,
    dto::{CreateMultipartUploadInput, CreateMultipartUploadOutput, StorageClass},
};

use crate::{S3Adapter, access::S3Access, metadata, to_key};

pub(crate) async fn create_multipart_upload_access(
    access: &S3Access,
    req: &mut S3Request<CreateMultipartUploadInput>,
) -> S3Result<()> {
    access
        .authorize(req.credentials.as_ref(), &to_key!(req.input), Action::Write)
        .await
}

pub(crate) async fn create_multipart_upload(
    adapter: &S3Adapter,
    req: S3Request<CreateMultipartUploadInput>,
    #[cfg(feature = "metrics")] metrics: Option<crate::metrics::RequestMetrics>,
) -> S3Result<S3Response<CreateMultipartUploadOutput>> {
    let input = req.input;
    validate_input(&input)?;

    let mut metadata = input
        .metadata
        .unwrap_or_default()
        .into_iter()
        .collect::<object_registry::Metadata>();
    metadata.insert(
        "content-type".to_owned(),
        input
            .content_type
            .unwrap_or_else(|| "application/octet-stream".to_owned()),
    );

    let blob_id = adapter
        .blobs
        .create()
        .await
        .map_err(S3Error::internal_error)?;

    // The registries do not share a transaction. A failure can leave an open
    // blob behind, and a flush error does not imply rollback.
    metadata::set_last_modified(&mut metadata)?;
    let upload_id = adapter
        .multipart
        .create(MultipartUpload {
            blob_id,
            bucket: input.bucket.clone(),
            key: input.key.clone(),
            metadata,
        })
        .await
        .map_err(S3Error::internal_error)?;

    success!(metrics);

    Ok(S3Response::new(CreateMultipartUploadOutput {
        bucket: Some(input.bucket),
        key: Some(input.key),
        upload_id: Some(upload_id.as_str().to_owned()),
        ..Default::default()
    }))
}

fn validate_input(input: &CreateMultipartUploadInput) -> S3Result<()> {
    if input.bucket.is_empty()
        || input.bucket.contains(['/', '\0'])
        || input.key.is_empty()
        || input.key.contains('\0')
    {
        return Err(s3s::s3_error!(InvalidArgument, "invalid bucket or key"));
    }

    if input.metadata.as_ref().is_some_and(|metadata| {
        metadata
            .keys()
            .any(|key| key.is_empty() || key.contains('\0') || metadata::is_reserved_key(key))
    }) {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid or reserved metadata key"
        ));
    }

    reject_unsupported!(
        "CreateMultipartUpload",
        input,
        acl,
        bucket_key_enabled,
        cache_control,
        checksum_algorithm,
        checksum_type,
        content_disposition,
        content_encoding,
        content_language,
        expected_bucket_owner,
        expires,
        grant_full_control,
        grant_read,
        grant_read_acp,
        grant_write_acp,
        object_lock_legal_hold_status,
        object_lock_mode,
        object_lock_retain_until_date,
        request_payer,
        server_side_encryption,
        sse_customer_algorithm,
        sse_customer_key,
        sse_customer_key_md5,
        ssekms_encryption_context,
        ssekms_key_id,
        tagging,
        website_redirect_location,
    );
    reject_unsupported!(
        "CreateMultipartUpload";
        storage_class => input
            .storage_class
            .as_ref()
            .is_some_and(|value| value.as_str() != StorageClass::STANDARD),
    );

    Ok(())
}

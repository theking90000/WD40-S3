use access_controller::Action;
use object_registry::{Metadata, VersionId, traits::ObjectRegistry as _};
use s3s::{
    S3Request, S3Response, S3Result,
    dto::{
        CopyObjectInput, CopyObjectOutput, CopyObjectResult, CopySource, MetadataDirective,
        StorageClass,
    },
};

use crate::{S3Adapter, access::S3Access, error, metadata, to_key};

pub(crate) async fn copy_object_access(
    access: &S3Access,
    req: &mut S3Request<CopyObjectInput>,
) -> S3Result<()> {
    let (source_key, _) = source_object(&req.input.copy_source)?;
    access
        .authorize(req.credentials.as_ref(), &source_key, Action::Read)
        .await?;
    access
        .authorize(req.credentials.as_ref(), &to_key!(req.input), Action::Write)
        .await
}

pub(crate) async fn copy_object(
    adapter: &S3Adapter,
    req: S3Request<CopyObjectInput>,
    #[cfg(feature = "metrics")] mut metrics: Option<crate::metrics::RequestMetrics>,
) -> S3Result<S3Response<CopyObjectOutput>> {
    let mut input = req.input;
    validate_input(&input)?;
    let (source_key, source_version) = source_object(&input.copy_source)?;
    let key = to_key!(input);

    // An explicit source version can restore an older version to the same key.
    if source_key == key
        && source_version.is_none()
        && input
            .metadata_directive
            .as_ref()
            .map(|value| value.as_str())
            != Some(MetadataDirective::REPLACE)
    {
        return Err(s3s::s3_error!(
            InvalidRequest,
            "copying an object onto itself requires REPLACE or an explicit source version"
        ));
    }

    let record = adapter
        .objects
        .head(&source_key, source_version)
        .await
        .map_err(error::object_registry_error)?;
    if record.is_delete_marker {
        return Err(if source_version.is_some() {
            s3s::s3_error!(InvalidRequest, "copy source version is a delete marker")
        } else {
            s3s::s3_error!(NoSuchKey)
        });
    }
    let blob_id = record
        .blob_id
        .ok_or_else(|| s3s::s3_error!(InternalError, "object has no blob reference"))?;
    let mut object_metadata = copy_metadata(&mut input, record.metadata)?;
    metadata::set_stored_credential(
        &mut object_metadata,
        req.credentials
            .as_ref()
            .map(|credential| credential.access_key.as_str()),
    );
    let last_modified = metadata::set_last_modified(&mut object_metadata)?;

    // Finalized blobs are immutable. A new object version can share their bytes.
    // A store flush error does not imply that the new version was rolled back.
    let version_id = adapter
        .store_object(
            &key,
            blob_id,
            object_metadata,
            #[cfg(feature = "metrics")]
            &mut metrics,
        )
        .await?
        .version_id;

    success!(metrics);

    Ok(S3Response::new(CopyObjectOutput {
        copy_object_result: Some(CopyObjectResult {
            e_tag: Some(metadata::etag_from_version(version_id)),
            last_modified: Some(last_modified),
            ..Default::default()
        }),
        copy_source_version_id: Some(record.version_id.to_string()),
        version_id: Some(version_id.to_string()),
        ..Default::default()
    }))
}

fn source_object(source: &CopySource) -> S3Result<(String, Option<VersionId>)> {
    let CopySource::Bucket {
        bucket,
        key,
        version_id,
    } = source
    else {
        return Err(s3s::s3_error!(
            NotImplemented,
            "unsupported copy source ARN"
        ));
    };
    if bucket.is_empty() || bucket.contains(['/', '\0']) || key.is_empty() || key.contains('\0') {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid source bucket or key"
        ));
    }
    let version_id = version_id
        .as_deref()
        .map(str::parse::<VersionId>)
        .transpose()
        .map_err(|error| s3s::s3_error!(error, InvalidArgument))?;
    Ok((format!("{bucket}/{key}"), version_id))
}

fn validate_input(input: &CopyObjectInput) -> S3Result<()> {
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
    match input
        .metadata_directive
        .as_ref()
        .map(|value| value.as_str())
    {
        None | Some(MetadataDirective::COPY | MetadataDirective::REPLACE) => {}
        Some(_) => {
            return Err(s3s::s3_error!(
                InvalidArgument,
                "invalid metadata directive"
            ));
        }
    }
    validate_options(input)
}

fn validate_options(input: &CopyObjectInput) -> S3Result<()> {
    super::put_object::validate_acl(input.acl.as_ref())?;
    reject_unsupported!(
        "CopyObject",
        input,
        annotation_directive,
        bucket_key_enabled,
        cache_control,
        checksum_algorithm,
        content_disposition,
        content_encoding,
        content_language,
        copy_source_if_match,
        copy_source_if_modified_since,
        copy_source_if_none_match,
        copy_source_if_unmodified_since,
        copy_source_sse_customer_algorithm,
        copy_source_sse_customer_key,
        copy_source_sse_customer_key_md5,
        expected_bucket_owner,
        expected_source_bucket_owner,
        expires,
        grant_full_control,
        grant_read,
        grant_read_acp,
        grant_write_acp,
        if_match,
        if_none_match,
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
        tagging_directive,
        website_redirect_location,
    );
    reject_unsupported!(
        "CopyObject";
        storage_class => input
            .storage_class
            .as_ref()
            .is_some_and(|value| value.as_str() != StorageClass::STANDARD),
    );
    Ok(())
}

fn copy_metadata(input: &mut CopyObjectInput, mut source: Metadata) -> S3Result<Metadata> {
    let size = metadata::take_size(&mut source)?;
    let content_type = metadata::take_content_type(&mut source)?;
    if size > 5_368_709_120 {
        return Err(s3s::s3_error!(EntityTooLarge, "copy source exceeds 5 GiB"));
    }

    let mut metadata = if input
        .metadata_directive
        .as_ref()
        .map(|value| value.as_str())
        == Some(MetadataDirective::REPLACE)
    {
        let mut replacement = input
            .metadata
            .take()
            .unwrap_or_default()
            .into_iter()
            .collect::<Metadata>();
        replacement.insert(
            "content-type".to_owned(),
            input
                .content_type
                .take()
                .unwrap_or_else(|| "application/octet-stream".to_owned()),
        );
        replacement
    } else {
        source.insert("content-type".to_owned(), content_type);
        source
    };
    metadata.insert("size".to_owned(), size.to_string());
    Ok(metadata)
}

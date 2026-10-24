use std::collections::BTreeMap;

use access_controller::Action;
use blob_system::{BlobPart, PartId};
use object_registry::traits::ObjectRegistry as _;
use s3_multipart_registry::{MultipartUploadId, traits::S3MultipartRegistry as _};
use s3s::{
    S3Error, S3Request, S3Response, S3Result,
    dto::{CompleteMultipartUploadInput, CompleteMultipartUploadOutput, CompletedPart, ETag},
};

use crate::{S3Adapter, access::S3Access, error, metadata, to_key};

const MIN_PART_SIZE: u64 = 5_242_880;
const MAX_PART_SIZE: u64 = 5_368_709_120;

pub(crate) async fn complete_multipart_upload_access(
    access: &S3Access,
    req: &mut S3Request<CompleteMultipartUploadInput>,
) -> S3Result<()> {
    access
        .authorize(req.credentials.as_ref(), &to_key!(req.input), Action::Write)
        .await
}

pub(crate) async fn complete_multipart_upload(
    adapter: &S3Adapter,
    req: S3Request<CompleteMultipartUploadInput>,
    #[cfg(feature = "metrics")] metrics: Option<crate::metrics::RequestMetrics>,
) -> S3Result<S3Response<CompleteMultipartUploadOutput>> {
    let mut input = req.input;
    let expected_size = validate_input(&input)?;
    let parts = input
        .multipart_upload
        .take()
        .and_then(|upload| upload.parts)
        .ok_or_else(|| s3s::s3_error!(InvalidRequest, "missing multipart parts"))?;
    let selected = validate_parts(parts)?;

    let upload_id = input
        .upload_id
        .parse::<MultipartUploadId>()
        .map_err(|_| s3s::s3_error!(NoSuchUpload))?;
    let mut upload = adapter
        .multipart
        .get(&upload_id)
        .await
        .map_err(S3Error::internal_error)?
        .ok_or_else(|| s3s::s3_error!(NoSuchUpload))?;
    if upload.bucket != input.bucket || upload.key != input.key {
        return Err(s3s::s3_error!(NoSuchUpload));
    }
    if !upload.metadata.contains_key("content-type") {
        return Err(s3s::s3_error!(
            InternalError,
            "missing object content-type metadata"
        ));
    }

    let registered = adapter
        .blobs
        .get_parts(upload.blob_id)
        .await
        .map_err(S3Error::internal_error)?;
    validate_stored_parts(&selected, &registered, expected_size)?;
    drop(registered);

    let part_ids = selected.iter().map(|(part_id, _)| *part_id).collect();
    let length = adapter
        .blobs
        .finalize(upload.blob_id, part_ids)
        .await
        .map_err(S3Error::internal_error)?;

    // Parts may have been replaced after the first read. Once finalized, their
    // references cannot change. Check them again before publishing the object.
    // A rejection here leaves the blob finalized and the session still present.
    let registered = adapter
        .blobs
        .get_parts(upload.blob_id)
        .await
        .map_err(S3Error::internal_error)?;
    let selected_length = validate_stored_parts(&selected, &registered, expected_size)?;
    if selected_length != length {
        return Err(s3s::s3_error!(
            InternalError,
            "finalized blob length mismatch"
        ));
    }
    drop(registered);

    // Preserve initialization metadata. The completion request's Content-Type
    // describes its XML body, not the content type of the stored object.
    upload
        .metadata
        .insert("size".to_owned(), length.to_string());
    // Keep the initiation date, with completion time for older sessions.
    if !upload.metadata.contains_key("last-modified") {
        metadata::set_last_modified(&mut upload.metadata)?;
    }
    let version_id = adapter
        .objects
        .store(&to_key!(input), upload.blob_id, upload.metadata)
        .await
        .map_err(error::object_registry_error)?;

    // These registries do not share a transaction. Flush failures can occur
    // after a commit, and a retry can publish another object version.
    adapter
        .multipart
        .remove(&upload_id)
        .await
        .map_err(S3Error::internal_error)?;

    success!(metrics);

    Ok(S3Response::new(CompleteMultipartUploadOutput {
        bucket: Some(input.bucket),
        key: Some(input.key),
        e_tag: Some(metadata::etag_from_version(version_id)),
        version_id: Some(version_id.to_string()),
        ..Default::default()
    }))
}

fn validate_input(input: &CompleteMultipartUploadInput) -> S3Result<Option<u64>> {
    if input.bucket.is_empty()
        || input.bucket.contains(['/', '\0'])
        || input.key.is_empty()
        || input.key.contains('\0')
    {
        return Err(s3s::s3_error!(InvalidArgument, "invalid bucket or key"));
    }
    reject_unsupported!(
        "CompleteMultipartUpload",
        input,
        checksum_crc32,
        checksum_crc32c,
        checksum_crc64nvme,
        checksum_md5,
        checksum_sha1,
        checksum_sha256,
        checksum_sha512,
        checksum_type,
        checksum_xxhash64,
        checksum_xxhash3,
        checksum_xxhash128,
        expected_bucket_owner,
        if_match,
        if_none_match,
        request_payer,
        sse_customer_algorithm,
        sse_customer_key,
        sse_customer_key_md5,
    );
    input
        .mpu_object_size
        .map(u64::try_from)
        .transpose()
        .map_err(|error| s3s::s3_error!(error, InvalidArgument))
}

fn validate_parts(parts: Vec<CompletedPart>) -> S3Result<Vec<(PartId, ETag)>> {
    if parts.is_empty() || parts.len() > 10_000 {
        return Err(s3s::s3_error!(
            InvalidRequest,
            "invalid multipart part count"
        ));
    }
    let mut selected = Vec::with_capacity(parts.len());
    let mut previous = 0;
    for part in parts {
        let number = part
            .part_number
            .ok_or_else(|| s3s::s3_error!(InvalidPart, "missing part number"))?;
        if !(1..=10_000).contains(&number) {
            return Err(s3s::s3_error!(InvalidArgument, "invalid part number"));
        }
        if number <= previous {
            return Err(s3s::s3_error!(InvalidPartOrder));
        }
        previous = number;
        reject_unsupported!(
            "CompleteMultipartUpload",
            part,
            checksum_crc32,
            checksum_crc32c,
            checksum_crc64nvme,
            checksum_md5,
            checksum_sha1,
            checksum_sha256,
            checksum_sha512,
            checksum_xxhash64,
            checksum_xxhash3,
            checksum_xxhash128,
        );
        let e_tag = part
            .e_tag
            .filter(|value| matches!(value, ETag::Strong(_)))
            .ok_or_else(|| s3s::s3_error!(InvalidPart, "missing or weak part ETag"))?;
        let part_id =
            PartId::try_from(number).map_err(|error| s3s::s3_error!(error, InvalidArgument))?;
        selected.push((part_id, e_tag));
    }
    Ok(selected)
}

fn validate_stored_parts(
    selected: &[(PartId, ETag)],
    registered: &BTreeMap<PartId, BlobPart>,
    expected_size: Option<u64>,
) -> S3Result<u64> {
    let last_part = selected.last().map(|(part_id, _)| *part_id);
    let mut length = 0_u64;
    for (part_id, e_tag) in selected {
        let part = registered
            .get(part_id)
            .ok_or_else(|| s3s::s3_error!(InvalidPart, "part {part_id} was not uploaded"))?;
        if !e_tag.strong_cmp(&ETag::Strong(part.stream_id.to_string())) {
            return Err(s3s::s3_error!(
                InvalidPart,
                "part {part_id} ETag does not match"
            ));
        }
        if part.length > MAX_PART_SIZE {
            return Err(s3s::s3_error!(EntityTooLarge));
        }
        if Some(*part_id) != last_part && part.length < MIN_PART_SIZE {
            return Err(s3s::s3_error!(EntityTooSmall));
        }
        length = length
            .checked_add(part.length)
            .ok_or_else(|| s3s::s3_error!(EntityTooLarge))?;
    }
    metadata::content_length(length)?;
    if expected_size.is_some_and(|expected| expected != length) {
        return Err(s3s::s3_error!(
            InvalidRequest,
            "multipart object size does not match"
        ));
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use s3s::S3ErrorCode;

    #[test]
    fn selected_parts_ignore_other_parts_and_only_the_last_may_be_small() -> S3Result<()> {
        let selected = validate_parts(vec![
            CompletedPart {
                part_number: Some(2),
                e_tag: Some(ETag::Strong("41".to_owned())),
                ..Default::default()
            },
            CompletedPart {
                part_number: Some(7),
                e_tag: Some(ETag::Strong("42".to_owned())),
                ..Default::default()
            },
        ])?;
        let mut registered = BTreeMap::from([
            (
                1,
                BlobPart {
                    stream_id: 40,
                    length: MAX_PART_SIZE,
                },
            ),
            (
                2,
                BlobPart {
                    stream_id: 41,
                    length: MIN_PART_SIZE,
                },
            ),
            (
                7,
                BlobPart {
                    stream_id: 42,
                    length: 0,
                },
            ),
        ]);
        if validate_stored_parts(&selected, &registered, Some(MIN_PART_SIZE))? != MIN_PART_SIZE {
            return Err(s3s::s3_error!(
                InternalError,
                "unselected part affected size"
            ));
        }
        registered.insert(
            2,
            BlobPart {
                stream_id: 41,
                length: 0,
            },
        );
        if !matches!(
            validate_stored_parts(&selected, &registered, None),
            Err(error) if error.code() == &S3ErrorCode::EntityTooSmall
        ) {
            return Err(s3s::s3_error!(
                InternalError,
                "small non-final part accepted"
            ));
        }
        Ok(())
    }

    #[test]
    fn same_length_replacement_is_rejected_by_the_second_check() -> S3Result<()> {
        let selected = vec![(1, ETag::Strong("41".to_owned()))];
        let mut registered = BTreeMap::from([(
            1,
            BlobPart {
                stream_id: 41,
                length: 3,
            },
        )]);
        validate_stored_parts(&selected, &registered, Some(3))?;
        registered.insert(
            1,
            BlobPart {
                stream_id: 42,
                length: 3,
            },
        );
        if !matches!(
            validate_stored_parts(&selected, &registered, Some(3)),
            Err(error) if error.code() == &S3ErrorCode::InvalidPart
        ) {
            return Err(s3s::s3_error!(InternalError, "replaced part ETag accepted"));
        }
        Ok(())
    }
}

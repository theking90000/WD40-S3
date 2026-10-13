use access_controller::Action;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use blob_system::BytesFrameAllocator;
use bytes::Bytes;
use futures_util::{StreamExt, io::AsyncWriteExt};
use object_registry::traits::ObjectRegistry as _;
use s3s::{
    S3Error, S3ErrorCode, S3Request, S3Response, S3Result, StdError,
    checksum::ChecksumHasher,
    crypto::{self, Checksum as _},
    dto::{Checksum, PutObjectInput, PutObjectOutput, StorageClass, StreamingBlob},
    stream::{aws_chunked_stream::AwsChunkedStreamError, upload_stream::UploadStreamError},
};

use crate::{S3Adapter, access::S3Access, error, metadata, to_key};

pub(crate) async fn put_object_access(
    access: &S3Access,
    req: &mut S3Request<PutObjectInput>,
) -> S3Result<()> {
    access
        .authorize(req.credentials.as_ref(), &to_key!(req.input), Action::Write)
        .await
}

fn validate_input(input: &PutObjectInput) -> S3Result<Option<u64>> {
    if input.bucket.is_empty()
        || input.bucket.contains(['/', '\0'])
        || input.key.is_empty()
        || input.key.contains('\0')
    {
        return Err(s3s::s3_error!(InvalidArgument, "invalid bucket or key"));
    }
    if input.metadata.as_ref().is_some_and(|metadata| {
        metadata.keys().any(|key| {
            key.is_empty()
                || key.contains('\0')
                || key.eq_ignore_ascii_case("size")
                || key.eq_ignore_ascii_case("content-type")
        })
    }) {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid or reserved metadata key"
        ));
    }
    if input.acl.is_some()
        || input.bucket_key_enabled.is_some()
        || input.cache_control.is_some()
        || input.content_disposition.is_some()
        || input.content_language.is_some()
        || input.expected_bucket_owner.is_some()
        || input.expires.is_some()
        || input.grant_full_control.is_some()
        || input.grant_read.is_some()
        || input.grant_read_acp.is_some()
        || input.grant_write_acp.is_some()
        || input.if_match.is_some()
        || input.if_none_match.is_some()
        || input.object_lock_legal_hold_status.is_some()
        || input.object_lock_mode.is_some()
        || input.object_lock_retain_until_date.is_some()
        || input.request_payer.is_some()
        || input.server_side_encryption.is_some()
        || input.sse_customer_algorithm.is_some()
        || input.sse_customer_key.is_some()
        || input.sse_customer_key_md5.is_some()
        || input.ssekms_encryption_context.is_some()
        || input.ssekms_key_id.is_some()
        || input.tagging.is_some()
        || input.website_redirect_location.is_some()
        || input.write_offset_bytes.is_some()
        || input
            .storage_class
            .as_ref()
            .is_some_and(|value| value.as_str() != StorageClass::STANDARD)
        || input.content_encoding.as_ref().is_some_and(|value| {
            value
                .split(',')
                .any(|encoding| !encoding.trim().eq_ignore_ascii_case("aws-chunked"))
        })
    {
        return Err(s3s::s3_error!(
            NotImplemented,
            "unsupported PutObject option"
        ));
    }
    input
        .content_length
        .map(u64::try_from)
        .transpose()
        .map_err(|error| s3s::s3_error!(error, InvalidArgument))
}

fn check_chunk(length: u64, chunk_length: usize, expected: Option<u64>) -> S3Result<()> {
    let chunk_length = u64::try_from(chunk_length).map_err(S3Error::internal_error)?;
    let next_length = length
        .checked_add(chunk_length)
        .ok_or_else(|| s3s::s3_error!(EntityTooLarge))?;
    if expected.is_some_and(|expected| next_length > expected) {
        return Err(s3s::s3_error!(
            IncompleteBody,
            "body exceeds declared length"
        ));
    }
    metadata::content_length(next_length)?;
    Ok(())
}

fn check_length(length: u64, expected: Option<u64>) -> S3Result<()> {
    if expected.is_some_and(|expected| length != expected) {
        return Err(s3s::s3_error!(
            IncompleteBody,
            "body does not match declared length"
        ));
    }
    Ok(())
}

fn body_error(error: StdError) -> S3Error {
    let error = match error.downcast::<S3Error>() {
        Ok(error) => return *error,
        Err(error) => error,
    };
    // ponytail: S3S exposes verification errors directly on the body stream.
    let code = if let Some(error) = error.downcast_ref::<AwsChunkedStreamError>() {
        error.to_s3_error_code()
    } else if let Some(error) = error.downcast_ref::<UploadStreamError>() {
        error.to_s3_error_code()
    } else {
        S3ErrorCode::InvalidRequest
    };
    S3Error::with_source(code, error)
}

pub(crate) async fn put_object(
    adapter: &S3Adapter,
    mut req: S3Request<PutObjectInput>,
) -> S3Result<S3Response<PutObjectOutput>> {
    let expected_length = validate_input(&req.input)?;
    let (mut hasher, trailer_names) = prepare_checksums(&req)?;
    let mut body = req
        .input
        .body
        .take()
        .unwrap_or_else(|| StreamingBlob::from_bytes(Bytes::new()));
    let first = loop {
        match body.next().await.transpose().map_err(body_error)? {
            Some(data) if data.is_empty() => continue,
            first => break first,
        }
    };

    let (blob_id, parts, length, checksum) = if let Some(first) = first {
        check_chunk(0, first.len(), expected_length)?;
        let mut scheduler = adapter.config.scheduler;
        let frame_size =
            usize::try_from(adapter.config.writer.frame_size).map_err(S3Error::internal_error)?;
        let mut allocator =
            BytesFrameAllocator::new(frame_size).map_err(S3Error::internal_error)?;
        let blob_id = adapter
            .blobs
            .create()
            .await
            .map_err(S3Error::internal_error)?;
        let mut writer = adapter
            .blobs
            .write_part(
                blob_id,
                1,
                adapter.config.writer,
                &mut allocator,
                &adapter.budget,
                &mut scheduler,
            )
            .await
            .map_err(S3Error::internal_error)?;
        let mut chunk = Some(first);
        while let Some(data) = chunk {
            if !data.is_empty() {
                writer
                    .write_all(&data)
                    .await
                    .map_err(S3Error::internal_error)?;
                hasher.update(&data);
            }
            drop(data);
            // Progress backend operations while the client is sending the next
            // chunk. Cancelling progress preserves operations in the writer.
            chunk = tokio::select! {
                biased;
                result = writer.progress() => {
                    result.map_err(S3Error::internal_error)?;
                    return Err(s3s::s3_error!(InternalError, "writer progress completed unexpectedly"));
                }
                next = body.next() => next,
            }.transpose().map_err(body_error)?;
            if let Some(data) = &chunk {
                check_chunk(writer.length(), data.len(), expected_length)?;
            }
        }
        // EOF matters even after all announced bytes were received: signature
        // verification and trailers can still fail on the final stream poll.
        let length = writer.length();
        check_length(length, expected_length)?;
        let checksum = verify_checksums(&req, hasher, &trailer_names)?;
        writer.close().await.map_err(S3Error::internal_error)?;
        drop(writer);
        (blob_id, vec![1], length, checksum)
    } else {
        check_length(0, expected_length)?;
        let checksum = verify_checksums(&req, hasher, &trailer_names)?;
        let blob_id = adapter
            .blobs
            .create()
            .await
            .map_err(S3Error::internal_error)?;
        (blob_id, Vec::new(), 0, checksum)
    };

    // Registry operations are not a distributed transaction. Errors can leave
    // persisted storage behind; a store flush error does not imply rollback.
    let finalized_length = adapter
        .blobs
        .finalize(blob_id, parts)
        .await
        .map_err(S3Error::internal_error)?;
    if finalized_length != length {
        return Err(s3s::s3_error!(
            InternalError,
            "finalized blob length mismatch"
        ));
    }
    let key = to_key!(req.input);
    let mut object_metadata = req
        .input
        .metadata
        .unwrap_or_default()
        .into_iter()
        .collect::<object_registry::Metadata>();
    object_metadata.insert("size".to_owned(), finalized_length.to_string());
    object_metadata.insert(
        "content-type".to_owned(),
        req.input
            .content_type
            .unwrap_or_else(|| "application/octet-stream".to_owned()),
    );
    let version_id = adapter
        .objects
        .store(&key, blob_id, object_metadata)
        .await
        .map_err(error::object_registry_error)?;

    Ok(S3Response::new(PutObjectOutput {
        e_tag: Some(metadata::etag_from_version(version_id)),
        version_id: Some(version_id.to_string()),
        checksum_crc32: checksum.checksum_crc32,
        checksum_crc32c: checksum.checksum_crc32c,
        checksum_crc64nvme: checksum.checksum_crc64nvme,
        checksum_md5: checksum.checksum_md5,
        checksum_sha1: checksum.checksum_sha1,
        checksum_sha256: checksum.checksum_sha256,
        checksum_sha512: checksum.checksum_sha512,
        checksum_xxhash64: checksum.checksum_xxhash64,
        checksum_xxhash3: checksum.checksum_xxhash3,
        checksum_xxhash128: checksum.checksum_xxhash128,
        ..Default::default()
    }))
}

// Keep algorithm selection, digest lengths and verification in the same table.
macro_rules! checksum_fields {
    ($apply:ident) => {
        $apply!(
            checksum_crc32,
            crc32,
            Crc32,
            "CRC32",
            "x-amz-checksum-crc32",
            4
        );
        $apply!(
            checksum_crc32c,
            crc32c,
            Crc32c,
            "CRC32C",
            "x-amz-checksum-crc32c",
            4
        );
        $apply!(
            checksum_crc64nvme,
            crc64nvme,
            Crc64Nvme,
            "CRC64NVME",
            "x-amz-checksum-crc64nvme",
            8
        );
        $apply!(checksum_md5, md5, Md5, "MD5", "x-amz-checksum-md5", 16);
        $apply!(checksum_sha1, sha1, Sha1, "SHA1", "x-amz-checksum-sha1", 20);
        $apply!(
            checksum_sha256,
            sha256,
            Sha256,
            "SHA256",
            "x-amz-checksum-sha256",
            32
        );
        $apply!(
            checksum_sha512,
            sha512,
            Sha512,
            "SHA512",
            "x-amz-checksum-sha512",
            64
        );
        $apply!(
            checksum_xxhash64,
            xxhash64,
            XxHash64,
            "XXHASH64",
            "x-amz-checksum-xxhash64",
            8
        );
        $apply!(
            checksum_xxhash3,
            xxhash3,
            XxHash3,
            "XXHASH3",
            "x-amz-checksum-xxhash3",
            8
        );
        $apply!(
            checksum_xxhash128,
            xxhash128,
            XxHash128,
            "XXHASH128",
            "x-amz-checksum-xxhash128",
            16
        );
    };
}

fn checksum_header(name: &str) -> Option<&'static str> {
    macro_rules! matches_header {
        ($field:ident, $hasher:ident, $type:ident, $algorithm:literal, $header:literal, $length:literal) => {
            if name.eq_ignore_ascii_case($header) {
                return Some($header);
            }
        };
    }
    checksum_fields!(matches_header);
    None
}

fn validate_digest(value: &str, length: usize) -> S3Result<()> {
    let mut digest = [0_u8; 64];
    let decoded_length = STANDARD
        .decode_slice(value, &mut digest)
        .map_err(|error| s3s::s3_error!(error, InvalidDigest))?;
    if decoded_length != length {
        return Err(s3s::s3_error!(InvalidDigest, "invalid checksum length"));
    }
    Ok(())
}

fn prepare_checksums(
    req: &S3Request<PutObjectInput>,
) -> S3Result<(ChecksumHasher, Vec<&'static str>)> {
    let mut trailer_names = Vec::new();
    for value in req.headers.get_all("x-amz-trailer").iter() {
        let value = value
            .to_str()
            .map_err(|error| s3s::s3_error!(error, InvalidArgument))?;
        for name in value.split(',') {
            let name = checksum_header(name.trim())
                .ok_or_else(|| s3s::s3_error!(NotImplemented, "unsupported trailer: {name}"))?;
            if trailer_names.contains(&name) {
                return Err(s3s::s3_error!(
                    InvalidRequest,
                    "duplicate trailer declaration"
                ));
            }
            trailer_names.push(name);
        }
    }
    if !trailer_names.is_empty() && req.trailing_headers.is_none() {
        return Err(s3s::s3_error!(
            InvalidRequest,
            "checksum trailers require a verified trailer stream"
        ));
    }
    let input = &req.input;
    let algorithm = input
        .checksum_algorithm
        .as_ref()
        .map(|value| value.as_str());
    let mut known_algorithm = algorithm.is_none();
    let mut hasher = ChecksumHasher::default();
    macro_rules! enable {
        ($field:ident, $hasher:ident, $type:ident, $algorithm:literal, $header:literal, $length:literal) => {
            let declared = trailer_names.contains(&$header);
            if algorithm == Some($algorithm) {
                known_algorithm = true;
                if input.$field.is_none() && !declared {
                    return Err(s3s::s3_error!(
                        InvalidRequest,
                        "missing checksum for requested algorithm"
                    ));
                }
            }
            if let Some(value) = input.$field.as_deref() {
                validate_digest(value, $length)?;
            }
            if input.$field.is_some() || declared {
                hasher.$hasher = Some(crypto::$type::new());
            }
        };
    }
    checksum_fields!(enable);
    if !known_algorithm {
        return Err(s3s::s3_error!(
            NotImplemented,
            "unsupported checksum algorithm"
        ));
    }
    if let Some(value) = input.content_md5.as_deref() {
        validate_digest(value, 16)?;
        if hasher.md5.is_none() {
            hasher.md5 = Some(crypto::Md5::new());
        }
    }
    Ok((hasher, trailer_names))
}

fn verify_digest(expected: &str, actual: Option<&str>) -> S3Result<()> {
    if actual != Some(expected) {
        return Err(s3s::s3_error!(BadDigest, "checksum does not match body"));
    }
    Ok(())
}

fn verify_checksums(
    req: &S3Request<PutObjectInput>,
    hasher: ChecksumHasher,
    trailer_names: &[&str],
) -> S3Result<Checksum> {
    let mut actual = hasher.finalize();
    let trailers = req
        .trailing_headers
        .as_ref()
        .and_then(|handle| handle.take());
    for name in trailer_names {
        if trailers
            .as_ref()
            .is_none_or(|headers| !headers.contains_key(*name))
        {
            return Err(s3s::s3_error!(
                IncompleteBody,
                "missing checksum trailer: {name}"
            ));
        }
    }
    if let Some(headers) = &trailers {
        for name in headers.keys() {
            if name.as_str().starts_with("x-amz-checksum-")
                && !trailer_names.contains(&name.as_str())
            {
                return Err(s3s::s3_error!(
                    InvalidRequest,
                    "undeclared checksum trailer"
                ));
            }
        }
    }
    let input = &req.input;
    macro_rules! verify_field {
        ($field:ident, $hasher:ident, $type:ident, $algorithm:literal, $header:literal, $length:literal) => {
            if let Some(expected) = input.$field.as_deref() {
                verify_digest(expected, actual.$field.as_deref())?;
            }
            if let Some(headers) = &trailers {
                let all_values = headers.get_all($header);
                let mut values = all_values.iter();
                if let Some(value) = values.next() {
                    if values.next().is_some() {
                        return Err(s3s::s3_error!(InvalidRequest, "duplicate checksum trailer"));
                    }
                    let value = value
                        .to_str()
                        .map_err(|error| s3s::s3_error!(error, InvalidDigest))?;
                    validate_digest(value, $length)?;
                    verify_digest(value, actual.$field.as_deref())?;
                }
            }
        };
    }
    checksum_fields!(verify_field);
    if let Some(expected) = input.content_md5.as_deref() {
        verify_digest(expected, actual.checksum_md5.as_deref())?;
        if input.checksum_md5.is_none() && !trailer_names.contains(&"x-amz-checksum-md5") {
            actual.checksum_md5 = None;
        }
    }
    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_errors_and_digest_bounds() -> S3Result<()> {
        let errors: [(StdError, S3ErrorCode); 3] = [
            (
                Box::new(s3s::s3_error!(AccessDenied)),
                S3ErrorCode::AccessDenied,
            ),
            (
                Box::new(AwsChunkedStreamError::SignatureMismatch),
                S3ErrorCode::SignatureDoesNotMatch,
            ),
            (
                Box::new(UploadStreamError::Incomplete),
                S3ErrorCode::IncompleteBody,
            ),
        ];
        for (error, expected) in errors {
            if body_error(error).code() != &expected {
                return Err(s3s::s3_error!(InternalError, "body error code changed"));
            }
        }
        validate_digest(&STANDARD.encode([0_u8; 64]), 64)?;
        if validate_digest(&STANDARD.encode([0_u8; 65]), 64).is_ok()
            || validate_digest("invalid!", 4).is_ok()
        {
            return Err(s3s::s3_error!(InternalError, "invalid digest accepted"));
        }
        Ok(())
    }
}

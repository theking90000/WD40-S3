use access_controller::Action;
use blob_system::{BytesFrameAllocator, PartId};
use bytes::Bytes;
use futures_util::{StreamExt, io::AsyncWriteExt};
use s3_multipart_registry::{MultipartUploadId, traits::S3MultipartRegistry as _};
use s3s::{
    S3Error, S3Request, S3Response, S3Result,
    dto::{ETag, StreamingBlob, UploadPartInput, UploadPartOutput},
};

use crate::{S3Adapter, access::S3Access, to_key};

use super::put_object::{body_error, check_length, prepare_checksums, verify_checksums};

const MAX_PART_SIZE: u64 = 5_368_709_120;

pub(crate) async fn upload_part_access(
    access: &S3Access,
    req: &mut S3Request<UploadPartInput>,
) -> S3Result<()> {
    access
        .authorize(req.credentials.as_ref(), &to_key!(req.input), Action::Write)
        .await
}

pub(crate) async fn upload_part(
    adapter: &S3Adapter,
    mut req: S3Request<UploadPartInput>,
    #[cfg(feature = "metrics")] mut metrics: Option<crate::metrics::RequestMetrics>,
) -> S3Result<S3Response<UploadPartOutput>> {
    let (part_id, expected_length) = validate_input(&req.input)?;
    let upload_id = req
        .input
        .upload_id
        .parse::<MultipartUploadId>()
        .map_err(|_| s3s::s3_error!(NoSuchUpload))?;
    let upload = adapter
        .multipart
        .get(&upload_id)
        .await
        .map_err(S3Error::internal_error)?
        .ok_or_else(|| s3s::s3_error!(NoSuchUpload))?;

    if upload.bucket != req.input.bucket || upload.key != req.input.key {
        return Err(s3s::s3_error!(NoSuchUpload));
    }

    let (mut hasher, trailer_names) = prepare_checksums(
        &req,
        req.input
            .checksum_algorithm
            .as_ref()
            .map(|value| value.as_str()),
        requested_checksums(&req.input),
        req.input.content_md5.as_deref(),
    )?;
    let mut body = req
        .input
        .body
        .take()
        .unwrap_or_else(|| StreamingBlob::from_bytes(Bytes::new()));

    let mut scheduler = adapter.config.scheduler;
    let frame_size =
        usize::try_from(adapter.config.writer.frame_size).map_err(S3Error::internal_error)?;
    let mut allocator = BytesFrameAllocator::new(frame_size).map_err(S3Error::internal_error)?;
    let mut writer = adapter
        .blobs
        .write_part(
            upload.blob_id,
            part_id,
            adapter.config.writer,
            &mut allocator,
            &adapter.budget,
            &mut scheduler,
        )
        .await
        .map_err(S3Error::internal_error)?;

    loop {
        let next = tokio::select! {
            biased;
            result = writer.progress() => {
                result.map_err(S3Error::internal_error)?;
                return Err(s3s::s3_error!(InternalError, "writer progress completed unexpectedly"));
            }
            next = body.next() => next,
        };
        let Some(data) = next.transpose().map_err(body_error)? else {
            break;
        };
        #[cfg(feature = "metrics")]
        if let Some(metrics) = metrics.as_mut() {
            metrics
                .add_uploaded_bytes(data.len())
                .map_err(S3Error::internal_error)?;
        }
        check_chunk(writer.length(), data.len(), expected_length)?;
        if !data.is_empty() {
            writer
                .write_all(&data)
                .await
                .map_err(S3Error::internal_error)?;
            hasher.update(&data);
        }
    }

    // Consume EOF and verify signatures, length and checksums before replacing
    // the registered part. A failed body leaves the previous reference intact.
    check_length(writer.length(), expected_length)?;
    let checksum = verify_checksums(
        &req,
        hasher,
        &trailer_names,
        requested_checksums(&req.input),
        req.input.content_md5.as_deref(),
    )?;
    let stream_id = writer.close().await.map_err(S3Error::internal_error)?;

    success!(metrics);

    Ok(S3Response::new(UploadPartOutput {
        e_tag: Some(ETag::Strong(stream_id.to_string())),
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

fn validate_input(input: &UploadPartInput) -> S3Result<(PartId, Option<u64>)> {
    if input.bucket.is_empty()
        || input.bucket.contains(['/', '\0'])
        || input.key.is_empty()
        || input.key.contains('\0')
        || !(1..=10_000).contains(&input.part_number)
    {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid bucket, key or part number"
        ));
    }
    reject_unsupported!(
        "UploadPart",
        input,
        expected_bucket_owner,
        request_payer,
        sse_customer_algorithm,
        sse_customer_key,
        sse_customer_key_md5,
    );
    let part_id = PartId::try_from(input.part_number).map_err(S3Error::internal_error)?;
    let length = input
        .content_length
        .map(u64::try_from)
        .transpose()
        .map_err(|error| s3s::s3_error!(error, InvalidArgument))?;
    if length.is_some_and(|length| length > MAX_PART_SIZE) {
        return Err(s3s::s3_error!(EntityTooLarge));
    }
    Ok((part_id, length))
}

fn check_chunk(length: u64, chunk_length: usize, expected: Option<u64>) -> S3Result<()> {
    let chunk_length = u64::try_from(chunk_length).map_err(S3Error::internal_error)?;
    let next = length
        .checked_add(chunk_length)
        .ok_or_else(|| s3s::s3_error!(EntityTooLarge))?;
    if next > MAX_PART_SIZE {
        return Err(s3s::s3_error!(EntityTooLarge));
    }
    if expected.is_some_and(|expected| next > expected) {
        return Err(s3s::s3_error!(
            IncompleteBody,
            "body exceeds declared length"
        ));
    }
    Ok(())
}

fn requested_checksums(input: &UploadPartInput) -> [Option<&str>; 10] {
    [
        input.checksum_crc32.as_deref(),
        input.checksum_crc32c.as_deref(),
        input.checksum_crc64nvme.as_deref(),
        input.checksum_md5.as_deref(),
        input.checksum_sha1.as_deref(),
        input.checksum_sha256.as_deref(),
        input.checksum_sha512.as_deref(),
        input.checksum_xxhash64.as_deref(),
        input.checksum_xxhash3.as_deref(),
        input.checksum_xxhash128.as_deref(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_bounds_and_lengths() -> S3Result<()> {
        let mut input = UploadPartInput {
            bucket: "bucket".to_owned(),
            key: "key".to_owned(),
            ..Default::default()
        };
        for number in [-1, 0, 10_001] {
            input.part_number = number;
            if validate_input(&input).is_ok() {
                return Err(s3s::s3_error!(
                    InternalError,
                    "invalid part number accepted"
                ));
            }
        }
        for number in [1, 10_000] {
            input.part_number = number;
            validate_input(&input)?;
        }
        check_chunk(MAX_PART_SIZE, 0, None)?;
        if check_chunk(MAX_PART_SIZE, 1, None).is_ok() || check_chunk(0, 1, Some(0)).is_ok() {
            return Err(s3s::s3_error!(
                InternalError,
                "invalid part length accepted"
            ));
        }
        Ok(())
    }
}

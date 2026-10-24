use std::{
    collections::BTreeMap,
    ops::Bound::{Excluded, Unbounded},
};

use access_controller::Action;
use blob_system::{BlobPart, PartId};
use s3_multipart_registry::{MultipartUploadId, traits::S3MultipartRegistry as _};
use s3s::{
    S3Error, S3Request, S3Response, S3Result,
    dto::{ETag, ListPartsInput, ListPartsOutput, Part, StorageClass},
};

use crate::{S3Adapter, access::S3Access, metadata, to_key};

const MAX_PARTS: i32 = 1_000;

pub(crate) async fn list_parts_access(
    access: &S3Access,
    req: &mut S3Request<ListPartsInput>,
) -> S3Result<()> {
    access
        .authorize(req.credentials.as_ref(), &to_key!(req.input), Action::List)
        .await
}

pub(crate) async fn list_parts(
    adapter: &S3Adapter,
    req: S3Request<ListPartsInput>,
) -> S3Result<S3Response<ListPartsOutput>> {
    let input = req.input;
    let (max_parts, marker) = validate_input(&input)?;
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

    // Listing reports registered parts without reserving them or checking the
    // minimum size required by CompleteMultipartUpload. Pages are not snapshots.
    let registered = adapter
        .blobs
        .get_parts(upload.blob_id)
        .await
        .map_err(S3Error::internal_error)?;
    let (parts, is_truncated, next_marker) = list_page(&registered, max_parts, marker)?;

    Ok(S3Response::new(ListPartsOutput {
        bucket: Some(input.bucket),
        key: Some(input.key),
        upload_id: Some(input.upload_id),
        max_parts: Some(max_parts),
        part_number_marker: Some(marker),
        next_part_number_marker: next_marker,
        is_truncated: Some(is_truncated),
        parts: Some(parts),
        storage_class: Some(StorageClass::from_static(StorageClass::STANDARD)),
        ..Default::default()
    }))
}

fn validate_input(input: &ListPartsInput) -> S3Result<(i32, i32)> {
    if input.bucket.is_empty()
        || input.bucket.contains(['/', '\0'])
        || input.key.is_empty()
        || input.key.contains('\0')
    {
        return Err(s3s::s3_error!(InvalidArgument, "invalid bucket or key"));
    }
    reject_unsupported!(
        "ListParts",
        input,
        expected_bucket_owner,
        request_payer,
        sse_customer_algorithm,
        sse_customer_key,
        sse_customer_key_md5,
    );
    let max_parts = input.max_parts.unwrap_or(MAX_PARTS);
    let marker = input.part_number_marker.unwrap_or(0);
    if max_parts < 0 || marker < 0 {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid listing parameters"
        ));
    }
    Ok((max_parts.min(MAX_PARTS), marker))
}

fn list_page(
    registered: &BTreeMap<PartId, BlobPart>,
    max_parts: i32,
    marker: i32,
) -> S3Result<(Vec<Part>, bool, Option<i32>)> {
    let limit =
        usize::try_from(max_parts).map_err(|error| s3s::s3_error!(error, InvalidArgument))?;
    let marker_id =
        PartId::try_from(marker).map_err(|error| s3s::s3_error!(error, InvalidArgument))?;
    let mut remaining = registered.range((Excluded(marker_id), Unbounded));
    let mut parts = Vec::with_capacity(limit);
    let mut next_marker = marker;

    for (&part_id, part) in remaining.by_ref().take(limit) {
        let number = i32::try_from(part_id).map_err(S3Error::internal_error)?;
        if !(1..=10_000).contains(&number) {
            return Err(s3s::s3_error!(
                InternalError,
                "invalid registered S3 part number"
            ));
        }
        parts.push(Part {
            part_number: Some(number),
            e_tag: Some(ETag::Strong(part.stream_id.to_string())),
            size: Some(metadata::content_length(part.length)?),
            ..Default::default()
        });
        next_marker = number;
    }

    // Read one extra reference to distinguish a full final page from a
    // truncated page. With a zero limit, the marker deliberately stays put.
    let is_truncated = remaining.next().is_some();
    Ok((parts, is_truncated, is_truncated.then_some(next_marker)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered_parts() -> BTreeMap<PartId, BlobPart> {
        BTreeMap::from([
            (
                1,
                BlobPart {
                    stream_id: 41,
                    length: 3,
                },
            ),
            (
                4,
                BlobPart {
                    stream_id: 42,
                    length: 0,
                },
            ),
            (
                9,
                BlobPart {
                    stream_id: 43,
                    length: 5,
                },
            ),
        ])
    }

    #[test]
    fn pagination_resumes_after_the_last_returned_number() -> S3Result<()> {
        let registered = registered_parts();
        let (first, truncated, next) = list_page(&registered, 2, 0)?;
        if first
            .iter()
            .filter_map(|part| part.part_number)
            .collect::<Vec<_>>()
            != vec![1, 4]
            || !truncated
            || next != Some(4)
        {
            return Err(s3s::s3_error!(InternalError, "first page changed"));
        }
        let (second, truncated, next) = list_page(&registered, 1, 4)?;
        if second
            .iter()
            .filter_map(|part| part.part_number)
            .collect::<Vec<_>>()
            != vec![9]
            || truncated
            || next.is_some()
        {
            return Err(s3s::s3_error!(
                InternalError,
                "full final page was truncated"
            ));
        }
        Ok(())
    }

    #[test]
    fn zero_limit_and_markers_after_the_end_do_not_advance() -> S3Result<()> {
        let registered = registered_parts();
        let (parts, truncated, next) = list_page(&registered, 0, 4)?;
        if !parts.is_empty() || !truncated || next != Some(4) {
            return Err(s3s::s3_error!(
                InternalError,
                "zero limit advanced the marker"
            ));
        }
        for (max_parts, marker) in [(2, 9), (0, 10)] {
            let (parts, truncated, next) = list_page(&registered, max_parts, marker)?;
            if !parts.is_empty() || truncated || next.is_some() {
                return Err(s3s::s3_error!(
                    InternalError,
                    "marker after end returned a page"
                ));
            }
        }
        let (parts, truncated, next) = list_page(&BTreeMap::new(), 2, 0)?;
        if !parts.is_empty() || truncated || next.is_some() {
            return Err(s3s::s3_error!(InternalError, "empty upload was truncated"));
        }
        Ok(())
    }

    #[test]
    fn replacement_etag_and_small_part_size_are_reported() -> S3Result<()> {
        let mut registered = registered_parts();
        registered.insert(
            4,
            BlobPart {
                stream_id: 99,
                length: 0,
            },
        );
        let (parts, _, _) = list_page(&registered, 1, 1)?;
        let part = parts
            .first()
            .ok_or_else(|| s3s::s3_error!(InternalError, "missing part"))?;
        if part.part_number != Some(4)
            || part.e_tag != Some(ETag::Strong("99".to_owned()))
            || part.size != Some(0)
        {
            return Err(s3s::s3_error!(
                InternalError,
                "replacement metadata changed"
            ));
        }
        Ok(())
    }
}

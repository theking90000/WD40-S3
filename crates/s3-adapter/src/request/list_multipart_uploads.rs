use access_controller::Decision;
use s3_multipart_registry::{Cursor, MultipartUploadId, traits::S3MultipartRegistry as _};
use s3s::{
    S3Request, S3Response, S3Result,
    dto::{
        CommonPrefix, EncodingType, ListMultipartUploadsInput, ListMultipartUploadsOutput,
        MultipartUpload, StorageClass,
    },
};

use crate::{S3Adapter, access::S3Access, error};

use super::list_objects_v2::{ListPolicy, common_prefix, encode_value, list_policy};

const PAGE_SIZE: usize = 1_000;
const MAX_UPLOADS: i32 = 1_000;

pub(crate) async fn list_multipart_uploads_access(
    access: &S3Access,
    req: &mut S3Request<ListMultipartUploadsInput>,
) -> S3Result<()> {
    let policy = list_policy(
        access,
        req.credentials.as_ref(),
        &req.input.bucket,
        req.input.prefix.as_deref(),
    )
    .await?;
    req.extensions.insert(policy);
    Ok(())
}

pub(crate) async fn list_multipart_uploads(
    adapter: &S3Adapter,
    mut req: S3Request<ListMultipartUploadsInput>,
) -> S3Result<S3Response<ListMultipartUploadsOutput>> {
    let (max_uploads, encode) = validate_input(&req.input)?;
    let mut cursor = initial_cursor(&req.input)?;
    let policy = match req.extensions.remove::<ListPolicy>() {
        Some(policy) => policy,
        None => {
            let access = S3Access::new(adapter.credentials.clone(), adapter.access.clone());
            list_policy(
                &access,
                req.credentials.as_ref(),
                &req.input.bucket,
                req.input.prefix.as_deref(),
            )
            .await?
        }
    };
    let input = req.input;
    let prefix = input.prefix.as_deref().unwrap_or_default();
    let has_key_marker = input.key_marker.is_some();
    let mut uploads = Vec::new();
    let mut common_prefixes = Vec::new();
    let mut last_group: Option<String> = None;
    let mut next_key_marker = None;
    let mut next_upload_id_marker = None;
    let mut count = 0_i32;
    let mut is_truncated = false;

    'pages: loop {
        let page = adapter
            .multipart
            .list(&input.bucket, prefix, cursor.as_ref(), Some(PAGE_SIZE))
            .await
            .map_err(error::multipart_registry_error)?;
        for record in page.records {
            let key = &record.upload.key;
            if record.upload.bucket != input.bucket || !key.starts_with(prefix) {
                return Err(s3s::s3_error!(
                    InternalError,
                    "listing returned an upload outside its scope"
                ));
            }
            let allowed = policy.authorize(&format!("{}/{}", input.bucket, key)) == Decision::Allow;
            let (include, group) = visible_group(&input, key, allowed, last_group.as_deref());
            if !include {
                continue;
            }
            if count == max_uploads {
                // Look ahead to a visible result, not merely another registry
                // record. Client markers still point to the last emitted entry.
                is_truncated = true;
                break 'pages;
            }
            if let Some(group) = group {
                common_prefixes.push(CommonPrefix {
                    prefix: Some(encode_value(group.clone(), encode)),
                });
                next_key_marker = Some(group.clone());
                next_upload_id_marker = None;
                last_group = Some(group);
            } else {
                next_key_marker = Some(key.clone());
                next_upload_id_marker = Some(record.upload_id.as_str().to_owned());
                uploads.push(MultipartUpload {
                    key: Some(encode_value(key.clone(), encode)),
                    upload_id: Some(record.upload_id.as_str().to_owned()),
                    storage_class: Some(StorageClass::from_static(StorageClass::STANDARD)),
                    ..Default::default()
                });
            }
            count = count
                .checked_add(1)
                .ok_or_else(|| s3s::s3_error!(InternalError, "listing count overflow"))?;
        }
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = Some(next);
    }

    if !is_truncated {
        next_key_marker = None;
        next_upload_id_marker = None;
    }
    Ok(S3Response::new(ListMultipartUploadsOutput {
        bucket: Some(input.bucket),
        prefix: Some(encode_value(prefix.to_owned(), encode)),
        delimiter: input.delimiter.map(|value| encode_value(value, encode)),
        encoding_type: input.encoding_type,
        key_marker: input.key_marker.map(|value| encode_value(value, encode)),
        upload_id_marker: if has_key_marker {
            input.upload_id_marker
        } else {
            None
        },
        max_uploads: Some(max_uploads),
        is_truncated: Some(is_truncated),
        next_key_marker: next_key_marker.map(|value| encode_value(value, encode)),
        next_upload_id_marker,
        uploads: Some(uploads),
        common_prefixes: Some(common_prefixes),
        ..Default::default()
    }))
}

fn validate_input(input: &ListMultipartUploadsInput) -> S3Result<(i32, bool)> {
    if input.bucket.is_empty()
        || input.bucket.contains(['/', '\0'])
        || input
            .prefix
            .as_ref()
            .is_some_and(|value| value.contains('\0'))
        || input
            .delimiter
            .as_ref()
            .is_some_and(|value| value.contains('\0'))
        || input
            .key_marker
            .as_ref()
            .is_some_and(|value| value.contains('\0'))
    {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid listing bucket or key parameters"
        ));
    }
    if input.expected_bucket_owner.is_some() || input.request_payer.is_some() {
        return Err(s3s::s3_error!(
            NotImplemented,
            "unsupported ListMultipartUploads option"
        ));
    }
    let max_uploads = input.max_uploads.unwrap_or(MAX_UPLOADS);
    if !(1..=MAX_UPLOADS).contains(&max_uploads) {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "max-uploads must be between 1 and 1,000"
        ));
    }
    let encode = match input.encoding_type.as_ref().map(EncodingType::as_str) {
        None => false,
        Some(EncodingType::URL) => true,
        Some(_) => return Err(s3s::s3_error!(InvalidArgument, "unsupported encoding type")),
    };
    Ok((max_uploads, encode))
}

fn initial_cursor(input: &ListMultipartUploadsInput) -> S3Result<Option<Cursor>> {
    let Some(key) = &input.key_marker else {
        // S3 ignores upload-id-marker when no key-marker was provided.
        return Ok(None);
    };
    let upload_id = input
        .upload_id_marker
        .as_deref()
        .map(str::parse::<MultipartUploadId>)
        .transpose()
        .map_err(error::multipart_registry_error)?;
    Ok(Some(Cursor {
        bucket: input.bucket.clone(),
        prefix: input.prefix.clone().unwrap_or_default(),
        key: key.clone(),
        upload_id,
    }))
}

fn visible_group(
    input: &ListMultipartUploadsInput,
    key: &str,
    allowed: bool,
    last_group: Option<&str>,
) -> (bool, Option<String>) {
    // Denied uploads must not reveal their key through a common prefix either.
    if !allowed {
        return (false, None);
    }
    let prefix = input.prefix.as_deref().unwrap_or_default();
    let delimiter = input.delimiter.as_deref().filter(|value| !value.is_empty());
    let group = common_prefix(key, prefix, delimiter);
    if group.as_deref().is_some_and(|group| {
        last_group == Some(group)
            || input
                .key_marker
                .as_deref()
                .is_some_and(|marker| group <= marker)
    }) {
        return (false, None);
    }
    (true, group)
}

#[cfg(test)]
mod tests {
    use super::*;
    use s3s::S3ErrorCode;

    fn input() -> ListMultipartUploadsInput {
        ListMultipartUploadsInput {
            bucket: "bucket".to_owned(),
            delimiter: None,
            encoding_type: None,
            expected_bucket_owner: None,
            key_marker: None,
            max_uploads: None,
            prefix: None,
            request_payer: None,
            upload_id_marker: None,
        }
    }

    #[test]
    fn marker_pairs_are_preserved_and_upload_marker_alone_is_ignored() -> S3Result<()> {
        let mut input = input();
        input.prefix = Some("p/".to_owned());
        input.upload_id_marker = Some("invalid".to_owned());
        if initial_cursor(&input)?.is_some() {
            return Err(s3s::s3_error!(
                InternalError,
                "upload marker alone was used"
            ));
        }
        input.key_marker = Some("p/a".to_owned());
        if !matches!(initial_cursor(&input), Err(error) if error.code() == &S3ErrorCode::InvalidArgument)
        {
            return Err(s3s::s3_error!(
                InternalError,
                "invalid upload marker accepted"
            ));
        }
        input.upload_id_marker = None;
        let cursor = initial_cursor(&input)?
            .ok_or_else(|| s3s::s3_error!(InternalError, "missing key cursor"))?;
        if cursor.key != "p/a" || cursor.prefix != "p/" || cursor.upload_id.is_some() {
            return Err(s3s::s3_error!(InternalError, "key-only cursor changed"));
        }
        input.upload_id_marker = Some("a".repeat(32));
        let cursor = initial_cursor(&input)?
            .ok_or_else(|| s3s::s3_error!(InternalError, "missing pair cursor"))?;
        if cursor.upload_id.as_ref().map(MultipartUploadId::as_str)
            != input.upload_id_marker.as_deref()
        {
            return Err(s3s::s3_error!(InternalError, "upload marker was changed"));
        }
        Ok(())
    }

    #[test]
    fn denied_uploads_and_repeated_groups_do_not_produce_results() -> S3Result<()> {
        let mut input = input();
        input.prefix = Some("root/".to_owned());
        input.delimiter = Some("/".to_owned());
        if visible_group(&input, "root/secret/file", false, None) != (false, None) {
            return Err(s3s::s3_error!(
                InternalError,
                "denied upload revealed a prefix"
            ));
        }
        if visible_group(&input, "root/dir/a", true, None) != (true, Some("root/dir/".to_owned()))
            || visible_group(&input, "root/dir/b", true, Some("root/dir/")) != (false, None)
        {
            return Err(s3s::s3_error!(InternalError, "group was lost or repeated"));
        }
        input.key_marker = Some("root/dir/".to_owned());
        if visible_group(&input, "root/dir/c", true, None) != (false, None)
            || visible_group(&input, "root/file", true, None) != (true, None)
        {
            return Err(s3s::s3_error!(
                InternalError,
                "group marker changed visible results"
            ));
        }
        Ok(())
    }

    #[test]
    fn listing_limits_and_url_encoding_are_validated() -> S3Result<()> {
        let mut input = input();
        if validate_input(&input)? != (1_000, false) {
            return Err(s3s::s3_error!(InternalError, "listing defaults changed"));
        }
        for limit in [-1, 0, 1_001] {
            input.max_uploads = Some(limit);
            if !matches!(validate_input(&input), Err(error) if error.code() == &S3ErrorCode::InvalidArgument)
            {
                return Err(s3s::s3_error!(
                    InternalError,
                    "invalid listing limit accepted"
                ));
            }
        }
        input.max_uploads = Some(1);
        input.encoding_type = Some(EncodingType::from_static(EncodingType::URL));
        let (_, encode) = validate_input(&input)?;
        if !encode || encode_value("p/é.txt".to_owned(), encode) != "p%2F%C3%A9.txt" {
            return Err(s3s::s3_error!(InternalError, "URL encoding changed"));
        }
        Ok(())
    }
}

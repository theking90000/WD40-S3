use std::sync::Arc;

use access_controller::{Action, CompiledPolicy, Decision};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use object_registry::{Cursor, traits::ObjectRegistry as _};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use s3s::{
    S3Error, S3Request, S3Response, S3Result,
    auth::Credentials,
    dto::{CommonPrefix, EncodingType, ListObjectsV2Input, ListObjectsV2Output, Object},
};
use serde::{Deserialize, Serialize};

use crate::{S3Adapter, access::S3Access, error, metadata};

const PAGE_SIZE: usize = 1_000;
const MAX_KEYS: i32 = 1_000;
const TOKEN_VERSION: u8 = 1;
const MAX_TOKEN_LENGTH: usize = 16_384;

#[derive(Clone)]
pub(crate) struct ListPolicy(Arc<CompiledPolicy>);

impl ListPolicy {
    pub(super) fn authorize(&self, key: &str) -> Decision {
        self.0.authorize(Action::List, key)
    }
}

/// The cursor points before a registry page. last_key resumes within that page.
/// Pages are not snapshots, so use a key bound rather than a record offset.
#[derive(Serialize, Deserialize)]
struct Continuation {
    version: u8,
    bucket: String,
    prefix: String,
    delimiter: Option<String>,
    start_after: Option<String>,
    cursor: Option<Cursor>,
    last_key: Option<String>,
    last_common_prefix: Option<String>,
}

fn registry_prefix(bucket: &str, prefix: Option<&str>) -> S3Result<String> {
    let prefix = prefix.unwrap_or_default();
    if bucket.is_empty() || bucket.contains(['/', '\0']) || prefix.contains('\0') {
        return Err(s3s::s3_error!(InvalidArgument, "invalid bucket or prefix"));
    }
    Ok(format!("{bucket}/{prefix}"))
}

async fn compile_policy(
    access: &S3Access,
    credentials: Option<&Credentials>,
    prefix: &str,
) -> S3Result<Arc<CompiledPolicy>> {
    let principal = access.get_principal(credentials).await?;
    let policy = access
        .access
        .compile(principal)
        .await
        .map_err(S3Error::internal_error)?;
    if policy.authorize(Action::List, prefix) != Decision::Allow {
        return Err(s3s::s3_error!(AccessDenied));
    }
    Ok(policy)
}

pub(crate) async fn list_objects_v2_access(
    access: &S3Access,
    req: &mut S3Request<ListObjectsV2Input>,
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

pub(crate) async fn list_policy(
    access: &S3Access,
    credentials: Option<&Credentials>,
    bucket: &str,
    prefix: Option<&str>,
) -> S3Result<ListPolicy> {
    let prefix = registry_prefix(bucket, prefix)?;
    Ok(ListPolicy(
        compile_policy(access, credentials, &prefix).await?,
    ))
}

fn validate_input(input: &ListObjectsV2Input, operation: &str) -> S3Result<(i32, bool)> {
    reject_unsupported!(
        operation;
        fetch_owner => input.fetch_owner == Some(true),
        expected_bucket_owner => input.expected_bucket_owner.is_some(),
        request_payer => input.request_payer.is_some(),
        optional_object_attributes => input
            .optional_object_attributes
            .as_ref()
            .is_some_and(|attributes| !attributes.is_empty()),
    );
    let max_keys = input.max_keys.unwrap_or(MAX_KEYS);
    if max_keys < 0
        || input
            .delimiter
            .as_ref()
            .is_some_and(|value| value.contains('\0'))
        || input
            .start_after
            .as_ref()
            .is_some_and(|value| value.contains('\0'))
    {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid listing parameters"
        ));
    }
    let encode = match input.encoding_type.as_ref().map(EncodingType::as_str) {
        None => false,
        Some(EncodingType::URL) => true,
        Some(_) => return Err(s3s::s3_error!(InvalidArgument, "unsupported encoding type")),
    };
    Ok((max_keys.min(MAX_KEYS), encode))
}

fn continuation(input: &ListObjectsV2Input) -> S3Result<Continuation> {
    let prefix = input.prefix.as_deref().unwrap_or_default();
    let delimiter = input.delimiter.as_deref().filter(|value| !value.is_empty());
    let Some(token) = input.continuation_token.as_ref() else {
        return Ok(Continuation {
            version: TOKEN_VERSION,
            bucket: input.bucket.clone(),
            prefix: prefix.to_owned(),
            delimiter: delimiter.map(str::to_owned),
            start_after: input.start_after.clone(),
            cursor: None,
            last_key: None,
            last_common_prefix: None,
        });
    };
    if token.len() > MAX_TOKEN_LENGTH {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "continuation token is too large"
        ));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|error| s3s::s3_error!(error, InvalidArgument))?;
    let (state, remaining) = postcard::take_from_bytes::<Continuation>(&bytes)
        .map_err(|error| s3s::s3_error!(error, InvalidArgument))?;
    if !remaining.is_empty()
        || state.version != TOKEN_VERSION
        || state.bucket != input.bucket
        || state.prefix != prefix
        || state.delimiter.as_deref() != delimiter
        || input
            .start_after
            .as_ref()
            .is_some_and(|key| state.start_after.as_ref() != Some(key))
        || state
            .start_after
            .as_ref()
            .is_some_and(|key| key.contains('\0'))
        || state.last_key.as_ref().is_some_and(|key| {
            !key.starts_with(&format!("{}/{prefix}", input.bucket)) || key.contains('\0')
        })
        || state.last_common_prefix.as_ref().is_some_and(|key| {
            !key.starts_with(prefix)
                || key.contains('\0')
                || delimiter.is_none_or(|delimiter| !key.ends_with(delimiter))
        })
    {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid continuation token"
        ));
    }
    Ok(state)
}

fn encode_token(state: &Continuation) -> S3Result<String> {
    let bytes = postcard::to_allocvec(state).map_err(S3Error::internal_error)?;
    let token = URL_SAFE_NO_PAD.encode(bytes);
    if token.len() > MAX_TOKEN_LENGTH {
        return Err(s3s::s3_error!(
            InternalError,
            "continuation token is too large"
        ));
    }
    Ok(token)
}

pub(super) fn encode_value(value: String, encode: bool) -> String {
    if encode {
        // Preserve RFC 3986 unreserved characters, including '-' and '_'.
        const SET: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~');
        utf8_percent_encode(&value, SET).to_string()
    } else {
        value
    }
}

pub(super) fn common_prefix(key: &str, prefix: &str, delimiter: Option<&str>) -> Option<String> {
    let delimiter = delimiter?;
    let suffix = key.strip_prefix(prefix)?;
    let (before, _) = suffix.split_once(delimiter)?;
    Some(format!("{prefix}{before}{delimiter}"))
}

pub(crate) async fn list_objects_v2(
    adapter: &S3Adapter,
    req: S3Request<ListObjectsV2Input>,
    #[cfg(feature = "metrics")] metrics: Option<crate::metrics::RequestMetrics>,
) -> S3Result<S3Response<ListObjectsV2Output>> {
    let (response, _) = list_page(adapter, req, true).await?;
    success!(metrics);
    Ok(response)
}

/// V1 resumes from a returned key or common prefix; V2 uses an opaque token.
pub(crate) async fn list_page(
    adapter: &S3Adapter,
    mut req: S3Request<ListObjectsV2Input>,
    continuation_tokens: bool,
) -> S3Result<(S3Response<ListObjectsV2Output>, Option<String>)> {
    let prefix = registry_prefix(&req.input.bucket, req.input.prefix.as_deref())?;
    let operation = if continuation_tokens {
        "ListObjectsV2"
    } else {
        "ListObjects"
    };
    let (max_keys, encode) = validate_input(&req.input, operation)?;
    let mut state = continuation(&req.input)?;
    let policy = match req.extensions.remove::<ListPolicy>() {
        Some(policy) => policy.0,
        None => {
            let access = S3Access::new(adapter.credentials.clone(), adapter.access.clone());
            compile_policy(&access, req.credentials.as_ref(), &prefix).await?
        }
    };
    let bucket_prefix = format!("{}/", req.input.bucket);
    let mut contents = Vec::new();
    let mut common_prefixes = Vec::new();
    let mut key_count = 0_i32;
    let mut is_truncated = false;
    let mut next_marker = None;

    if max_keys > 0 {
        'pages: loop {
            let page = adapter
                .objects
                .list(&prefix, state.cursor.as_ref(), Some(PAGE_SIZE), false)
                .await
                .map_err(error::object_registry_error)?;
            for mut record in page.records {
                if state
                    .last_key
                    .as_ref()
                    .is_some_and(|last| record.key.as_str() <= last.as_str())
                {
                    continue;
                }
                let key = record.key.strip_prefix(&bucket_prefix).ok_or_else(|| {
                    s3s::s3_error!(InternalError, "listing returned a key outside the bucket")
                })?;
                let group = common_prefix(key, &state.prefix, state.delimiter.as_deref());
                let after_start = state.start_after.as_ref().is_none_or(|start| {
                    key > start.as_str()
                        && group
                            .as_ref()
                            .is_none_or(|group| group.as_str() > start.as_str())
                });
                let repeated_group = group
                    .as_ref()
                    .is_some_and(|group| state.last_common_prefix.as_ref() == Some(group));
                if record.is_delete_marker
                    || policy.authorize(Action::List, &record.key) != Decision::Allow
                    || !after_start
                    || repeated_group
                {
                    state.last_key = Some(record.key);
                    continue;
                }
                if key_count == max_keys {
                    // Leave this result unread. The next request scans from the
                    // same page cursor and skips only previously consumed keys.
                    is_truncated = true;
                    break 'pages;
                }
                if !continuation_tokens && Some(key_count) == max_keys.checked_sub(1) {
                    // Use the last returned entry, never a filtered-out key
                    // consumed while looking ahead for another visible result.
                    let marker = group.as_deref().map_or(key, |group| group);
                    next_marker = Some(encode_value(marker.to_owned(), encode));
                }
                if let Some(group) = group {
                    common_prefixes.push(CommonPrefix {
                        prefix: Some(encode_value(group.clone(), encode)),
                    });
                    state.last_common_prefix = Some(group);
                } else {
                    let size = metadata::take_size(&mut record.metadata)?;
                    let last_modified = metadata::take_last_modified(&mut record.metadata)?;
                    contents.push(Object {
                        key: Some(encode_value(key.to_owned(), encode)),
                        size: Some(metadata::content_length(size)?),
                        e_tag: Some(metadata::etag_from_version(record.version_id)),
                        last_modified: Some(last_modified),
                        ..Default::default()
                    });
                }
                state.last_key = Some(record.key);
                key_count = key_count
                    .checked_add(1)
                    .ok_or_else(|| s3s::s3_error!(InternalError, "listing count overflow"))?;
            }
            let Some(next_cursor) = page.next_cursor else {
                break;
            };
            state.cursor = Some(next_cursor);
        }
    }

    let next_continuation_token = if is_truncated && continuation_tokens {
        Some(encode_token(&state)?)
    } else {
        None
    };
    let response = S3Response::new(ListObjectsV2Output {
        name: Some(req.input.bucket),
        prefix: Some(encode_value(state.prefix, encode)),
        max_keys: Some(max_keys),
        key_count: Some(key_count),
        continuation_token: req.input.continuation_token,
        is_truncated: Some(is_truncated),
        next_continuation_token,
        contents: Some(contents),
        common_prefixes: Some(common_prefixes),
        delimiter: req.input.delimiter.map(|value| encode_value(value, encode)),
        encoding_type: req.input.encoding_type,
        start_after: req
            .input
            .start_after
            .map(|value| encode_value(value, encode)),
        ..Default::default()
    });
    Ok((response, next_marker))
}

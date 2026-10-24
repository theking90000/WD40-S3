//! Lists explicitly named buckets with LIST access on `bucket/` from policies
//! directly attached to the authenticated principal. Global selectors are excluded.
//! Policy resources provide names even for empty buckets. Broad prefixes without
//! a slash cannot enumerate bucket names without a separate bucket catalogue.

use std::collections::BTreeSet;

use access_controller::{Action, Decision, Principal};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use policy_registry::ResourcePattern;
use s3s::{
    S3Error, S3Request, S3Response, S3Result,
    auth::Credentials,
    dto::{Bucket, ListBucketsInput, ListBucketsOutput, Timestamp, TimestampFormat},
};
use serde::{Deserialize, Serialize};

use crate::{S3Adapter, access::S3Access};

const MAX_BUCKETS: i32 = 10_000;
const TOKEN_VERSION: u8 = 1;
const MAX_TOKEN_LENGTH: usize = 1_024;

#[derive(Clone)]
struct ListedBuckets {
    principal_id: u128,
    names: BTreeSet<String>,
}

#[derive(Deserialize, Serialize)]
struct Continuation {
    version: u8,
    principal_id: u128,
    prefix: String,
    last_bucket: String,
}

pub(crate) async fn list_buckets_access(
    access: &S3Access,
    req: &mut S3Request<ListBucketsInput>,
) -> S3Result<()> {
    let buckets = load_buckets(access, req.credentials.as_ref()).await?;
    req.extensions.insert(buckets);
    Ok(())
}

async fn load_buckets(
    access: &S3Access,
    credentials: Option<&Credentials>,
) -> S3Result<ListedBuckets> {
    let principal = access.get_principal(credentials).await?;
    let Principal::Authenticated(principal_id) = principal else {
        return Err(s3s::s3_error!(AccessDenied));
    };
    let snapshot = access
        .access
        .load_principal_policies(principal_id)
        .await
        .map_err(S3Error::internal_error)?;
    let mut names = BTreeSet::new();
    for (_, policy) in &snapshot.policies {
        for statement in &policy.statements {
            let resource = match &statement.resource {
                ResourcePattern::Exact(resource) | ResourcePattern::Prefix(resource) => resource,
            };
            if let Some((bucket, _)) = resource.split_once('/') {
                if !bucket.is_empty() && !bucket.contains('\0') {
                    names.insert(bucket.to_owned());
                }
            }
        }
    }
    let policy = access
        .access
        .compile_snapshot(snapshot)
        .map_err(S3Error::internal_error)?;
    names.retain(|bucket| policy.authorize(Action::List, &format!("{bucket}/")) == Decision::Allow);
    Ok(ListedBuckets {
        principal_id,
        names,
    })
}

pub(crate) async fn list_buckets(
    adapter: &S3Adapter,
    mut req: S3Request<ListBucketsInput>,
) -> S3Result<S3Response<ListBucketsOutput>> {
    let buckets = match req.extensions.remove::<ListedBuckets>() {
        Some(buckets) => buckets,
        None => {
            let access = S3Access::new(adapter.credentials.clone(), adapter.access.clone());
            load_buckets(&access, req.credentials.as_ref()).await?
        }
    };
    Ok(S3Response::new(list_page(req.input, buckets)?))
}

fn list_page(input: ListBucketsInput, buckets: ListedBuckets) -> S3Result<ListBucketsOutput> {
    reject_unsupported!("ListBuckets", input, bucket_region);
    let max_buckets = input.max_buckets.unwrap_or(MAX_BUCKETS);
    let prefix = input.prefix.as_deref().unwrap_or_default();
    if !(1..=MAX_BUCKETS).contains(&max_buckets) || prefix.contains('\0') {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid listing parameters"
        ));
    }
    let limit = usize::try_from(max_buckets).map_err(S3Error::internal_error)?;
    let last_bucket = continuation(&input, buckets.principal_id)?;
    // Bucket creation dates are not stored; use the legacy object fallback.
    let creation_date = Timestamp::parse(TimestampFormat::DateTime, "2025-09-10T18:23:00.000Z")
        .map_err(S3Error::internal_error)?;
    let mut names = buckets.names.into_iter().filter(|name| {
        name.starts_with(prefix) && last_bucket.as_ref().is_none_or(|last| name > last)
    });
    let page: Vec<Bucket> = names
        .by_ref()
        .take(limit)
        .map(|name| Bucket {
            name: Some(name),
            creation_date: Some(creation_date.clone()),
            ..Default::default()
        })
        .collect();
    let continuation_token = if names.next().is_some() {
        let last = page
            .last()
            .and_then(|bucket| bucket.name.as_ref())
            .ok_or_else(|| s3s::s3_error!(InternalError, "missing last bucket"))?;
        Some(encode_token(&Continuation {
            version: TOKEN_VERSION,
            principal_id: buckets.principal_id,
            prefix: prefix.to_owned(),
            last_bucket: last.clone(),
        })?)
    } else {
        None
    };
    Ok(ListBucketsOutput {
        buckets: Some(page),
        continuation_token,
        prefix: input.prefix,
        ..Default::default()
    })
}

fn continuation(input: &ListBucketsInput, principal_id: u128) -> S3Result<Option<String>> {
    let Some(token) = &input.continuation_token else {
        return Ok(None);
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
        || state.principal_id != principal_id
        || state.prefix != input.prefix.as_deref().unwrap_or_default()
        || state.last_bucket.is_empty()
        || state.last_bucket.contains(['/', '\0'])
        || !state.last_bucket.starts_with(&state.prefix)
    {
        return Err(s3s::s3_error!(
            InvalidArgument,
            "invalid continuation token"
        ));
    }
    Ok(Some(state.last_bucket))
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

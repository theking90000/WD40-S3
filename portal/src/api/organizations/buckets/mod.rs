mod create;
mod list;
mod update;

use axum::{
    Router,
    routing::{get, patch},
};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use portal_db::{Bucket, BucketAccess, DatabaseConnection};
use serde::{Deserialize, Serialize};

use super::{Error, current::CurrentOrganization};

// Match s3-adapter's bucket segment encoding and preserve the trailing delimiter.
const SEGMENT: &AsciiSet = &CONTROLS.add(b'/').add(b'%');

pub(super) fn metric_prefix(name: &str) -> String {
    format!("/by-buckets/{}/", utf8_percent_encode(name, SEGMENT))
}

pub(super) fn router() -> Router<DatabaseConnection> {
    Router::new()
        .route("/buckets", get(list::handle).post(create::handle))
        .route("/buckets/{bucket_name}", patch(update::handle))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Access {
    Private,
    Public,
    PublicList,
}

impl From<Access> for BucketAccess {
    fn from(access: Access) -> Self {
        match access {
            Access::Private => Self::Private,
            Access::Public => Self::Public,
            Access::PublicList => Self::PublicList,
        }
    }
}

impl From<BucketAccess> for Access {
    fn from(access: BucketAccess) -> Self {
        match access {
            BucketAccess::Private => Self::Private,
            BucketAccess::Public => Self::Public,
            BucketAccess::PublicList => Self::PublicList,
        }
    }
}

#[derive(Serialize)]
pub(super) struct BucketResponse {
    organization_id: String,
    name: String,
    created_at: String,
    access: Access,
}

impl From<Bucket> for BucketResponse {
    fn from(bucket: Bucket) -> Self {
        Self {
            organization_id: bucket.organization_id.to_string(),
            name: bucket.name,
            created_at: bucket.created_at.to_rfc3339(),
            access: bucket.access.into(),
        }
    }
}

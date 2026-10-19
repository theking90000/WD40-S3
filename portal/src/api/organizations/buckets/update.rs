use axum::{
    Extension, Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, bucket,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait},
};
use serde::Deserialize;

use super::super::pending;
use super::{Access, BucketResponse, CurrentOrganization, Error};
use crate::sync::{self, SyncContext};

#[derive(Deserialize)]
pub(super) struct BucketPath {
    bucket_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    access: Access,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    Extension(sync): Extension<SyncContext>,
    path: Result<Path<BucketPath>, PathRejection>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<Json<BucketResponse>, Error> {
    organization.require_owner()?;
    let Path(path) = path?;
    let Json(request) = payload?;
    let transaction = db.begin().await?;

    let result = bucket::Entity::update_many()
        .set(bucket::ActiveModel {
            access: Set(request.access.into()),
            ..Default::default()
        })
        .filter(bucket::Column::OrganizationId.eq(organization.id))
        .filter(bucket::Column::Name.eq(path.bucket_name.clone()))
        .exec(&transaction)
        .await?;
    if result.rows_affected == 0 {
        return Err(Error::new(StatusCode::NOT_FOUND, "bucket_not_found"));
    }

    let state = pending::advance_bucket(&transaction, &path.bucket_name).await?;
    if state.applied_generation == 0 {
        pending::all_credentials(&transaction, organization.id).await?;
    }
    let bucket = bucket::Entity::find_by_id(path.bucket_name)
        .filter(bucket::Column::OrganizationId.eq(organization.id))
        .one(&transaction)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "bucket_not_found"))?;
    transaction.commit().await?;

    let applied = sync::sync_bucket(&sync, &bucket.name)
        .await
        .map_err(|error| Error::synchronization(error, &bucket.name))?;
    pending::require_applied(applied, state.generation, &bucket.name)?;

    Ok(Json(BucketResponse::from(bucket)))
}

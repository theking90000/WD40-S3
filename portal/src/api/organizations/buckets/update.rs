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
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set},
};
use serde::Deserialize;

use super::{Access, BucketResponse, CurrentOrganization, Error};

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
    path: Result<Path<BucketPath>, PathRejection>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<Json<BucketResponse>, Error> {
    organization.require_owner()?;
    let Path(path) = path?;
    let Json(request) = payload?;

    let result = bucket::Entity::update_many()
        .set(bucket::ActiveModel {
            access: Set(request.access.into()),
            ..Default::default()
        })
        .filter(bucket::Column::OrganizationId.eq(organization.id))
        .filter(bucket::Column::Name.eq(path.bucket_name.clone()))
        .exec(&db)
        .await?;
    if result.rows_affected == 0 {
        return Err(Error::new(StatusCode::NOT_FOUND, "bucket_not_found"));
    }

    let bucket = bucket::Entity::find_by_id(path.bucket_name)
        .filter(bucket::Column::OrganizationId.eq(organization.id))
        .one(&db)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "bucket_not_found"))?;

    Ok(Json(BucketResponse::from(bucket)))
}

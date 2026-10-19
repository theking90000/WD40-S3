use axum::{Extension, Json, extract::State};
use portal_db::{
    DatabaseConnection, bucket,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder},
};

use super::{BucketResponse, CurrentOrganization, Error};

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
) -> Result<Json<Vec<BucketResponse>>, Error> {
    let buckets = bucket::Entity::find()
        .filter(bucket::Column::OrganizationId.eq(organization.id))
        .order_by_asc(bucket::Column::Name)
        .all(&db)
        .await?;

    Ok(Json(
        buckets.into_iter().map(BucketResponse::from).collect(),
    ))
}

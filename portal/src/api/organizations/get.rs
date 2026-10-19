use axum::{Extension, Json, extract::State, http::StatusCode};
use portal_db::{
    DatabaseConnection, bucket, credential, organization, organization_member,
    sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter},
};
use serde::Serialize;

use super::{Error, OrganizationResponse, current::CurrentOrganization};

#[derive(Serialize)]
pub(super) struct OrganizationDetailsResponse {
    #[serde(flatten)]
    organization: OrganizationResponse,
    counts: OrganizationCounts,
}

#[derive(Serialize)]
struct OrganizationCounts {
    buckets: u64,
    credentials: u64,
    members: u64,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(current): Extension<CurrentOrganization>,
) -> Result<Json<OrganizationDetailsResponse>, Error> {
    let organization = organization::Entity::find_by_id(current.id)
        .one(&db)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "organization_not_found"))?;
    let buckets = bucket::Entity::find()
        .filter(bucket::Column::OrganizationId.eq(current.id))
        .count(&db)
        .await?;
    let credentials = credential::Entity::find()
        .filter(credential::Column::OrganizationId.eq(current.id))
        .count(&db)
        .await?;
    let members = organization_member::Entity::find()
        .filter(organization_member::Column::OrganizationId.eq(current.id))
        .count(&db)
        .await?;

    Ok(Json(OrganizationDetailsResponse {
        organization: OrganizationResponse::new(
            organization.id,
            organization.name,
            organization.created_at,
            current.access,
        ),
        counts: OrganizationCounts {
            buckets,
            credentials,
            members,
        },
    }))
}

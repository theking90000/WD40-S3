use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, organization,
    sea_orm::{ActiveModelTrait, EntityTrait, Set},
};
use serde::Deserialize;

use super::{Error, OrganizationResponse, current::CurrentOrganization, name_write_error, validate_name};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    name: String,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(current): Extension<CurrentOrganization>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<Json<OrganizationResponse>, Error> {
    current.require_owner()?;
    let Json(request) = payload?;
    let name = validate_name(&request.name)?;
    let organization = organization::Entity::find_by_id(current.id)
        .one(&db)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "organization_not_found"))?;
    let mut organization: organization::ActiveModel = organization.into();
    organization.name = Set(name.to_owned());
    let organization = organization.update(&db).await.map_err(name_write_error)?;

    Ok(Json(OrganizationResponse::new(
        organization.id,
        organization.name,
        organization.created_at,
        current.access,
    )))
}

use axum::{
    Extension, Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, credential,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set},
};
use serde::Deserialize;

use super::{
    AccessType, CredentialPath, CredentialResponse, CurrentOrganization, Error, validate_key_id,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    access_type: AccessType,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    path: Result<Path<CredentialPath>, PathRejection>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<Json<CredentialResponse>, Error> {
    organization.require_owner()?;
    let Path(path) = path?;
    validate_key_id(&path.key_id)?;
    let Json(request) = payload?;

    let result = credential::Entity::update_many()
        .set(credential::ActiveModel {
            access_type: Set(request.access_type.into()),
            ..Default::default()
        })
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .filter(credential::Column::KeyId.eq(path.key_id.clone()))
        .exec(&db)
        .await?;
    if result.rows_affected == 0 {
        return Err(Error::new(StatusCode::NOT_FOUND, "credential_not_found"));
    }

    let credential = credential::Entity::find_by_id(path.key_id)
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .one(&db)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "credential_not_found"))?;

    Ok(Json(CredentialResponse::from(credential)))
}

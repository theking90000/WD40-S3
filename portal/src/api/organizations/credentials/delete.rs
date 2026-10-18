use axum::{
    Extension,
    extract::{Path, State, rejection::PathRejection},
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, credential,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter},
};

use super::{CredentialPath, CurrentOrganization, Error, validate_key_id};

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    path: Result<Path<CredentialPath>, PathRejection>,
) -> Result<StatusCode, Error> {
    organization.require_owner()?;
    let Path(path) = path?;
    validate_key_id(&path.key_id)?;

    let result = credential::Entity::delete_many()
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .filter(credential::Column::KeyId.eq(path.key_id))
        .exec(&db)
        .await?;
    if result.rows_affected == 0 {
        return Err(Error::new(StatusCode::NOT_FOUND, "credential_not_found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

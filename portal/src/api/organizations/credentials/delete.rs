use axum::{
    Extension,
    extract::{Path, State, rejection::PathRejection},
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, credential,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait},
};

use super::super::pending;
use super::{CredentialPath, CurrentOrganization, Error, validate_key_id};
use crate::sync::{self, SyncContext};

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    Extension(sync): Extension<SyncContext>,
    path: Result<Path<CredentialPath>, PathRejection>,
) -> Result<StatusCode, Error> {
    organization.require_owner()?;
    let Path(path) = path?;
    validate_key_id(&path.key_id)?;

    let transaction = db.begin().await?;
    credential::Entity::find()
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .filter(credential::Column::KeyId.eq(path.key_id.clone()))
        .one(&transaction)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "credential_not_found"))?;
    pending::delete_credential(&transaction, &path.key_id).await?;
    transaction.commit().await?;

    sync::delete_credential(&sync, &path.key_id)
        .await
        .map_err(|error| Error::synchronization(error, &path.key_id))?;

    Ok(StatusCode::NO_CONTENT)
}

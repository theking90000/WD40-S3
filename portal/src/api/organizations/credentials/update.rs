use axum::{
    Extension, Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, credential, credential_bucket,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait, sea_query::Expr},
};

use super::super::pending;
use super::{
    CredentialPath, CredentialResponse, CurrentOrganization, Error, Request, selection,
    validate_key_id,
};
use crate::sync::{self, SyncContext};

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    Extension(sync): Extension<SyncContext>,
    path: Result<Path<CredentialPath>, PathRejection>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<Json<CredentialResponse>, Error> {
    organization.require_owner()?;
    let Path(path) = path?;
    validate_key_id(&path.key_id)?;
    let Json(request) = payload?;
    let transaction = db.begin().await?;

    // Lock before reading so a partial PATCH cannot restore older permissions.
    let result = credential::Entity::update_many()
        .col_expr(
            credential::Column::KeyId,
            Expr::col(credential::Column::KeyId).into(),
        )
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .filter(credential::Column::KeyId.eq(path.key_id.clone()))
        .exec(&transaction)
        .await?;
    if result.rows_affected == 0 {
        return Err(Error::new(StatusCode::NOT_FOUND, "credential_not_found"));
    }

    let current = credential::Entity::find_by_id(path.key_id.clone())
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .one(&transaction)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "credential_not_found"))?;
    let existing_bucket_names = credential_bucket::Entity::find()
        .filter(credential_bucket::Column::OrganizationId.eq(organization.id))
        .filter(credential_bucket::Column::KeyId.eq(path.key_id.clone()))
        .all(&transaction)
        .await?
        .into_iter()
        .map(|association| association.bucket_name)
        .collect();
    let (access_type, bucket_names, permissions) =
        request.into_selection(Some(&current), existing_bucket_names)?;
    credential::Entity::update_many()
        .set(credential::ActiveModel {
            access_type: Set(access_type),
            permissions: Set(permissions),
            ..Default::default()
        })
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .filter(credential::Column::KeyId.eq(path.key_id.clone()))
        .exec(&transaction)
        .await?;

    selection::validate(&transaction, organization.id, &bucket_names).await?;
    selection::replace(&transaction, organization.id, &path.key_id, &bucket_names).await?;
    pending::selected_buckets(&transaction, organization.id, access_type, &bucket_names).await?;
    let generation = pending::advance_credential(&transaction, &path.key_id).await?;

    let credential = credential::Entity::find_by_id(path.key_id)
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .one(&transaction)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "credential_not_found"))?;
    transaction.commit().await?;

    let applied = sync::sync_credential(&sync, &credential.key_id)
        .await
        .map_err(|error| Error::synchronization(error, &credential.key_id))?;
    pending::require_applied(applied, generation, &credential.key_id)?;

    Ok(Json(CredentialResponse::new(credential, bucket_names)))
}

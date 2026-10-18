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
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait},
};

use super::{
    CredentialPath, CredentialResponse, CurrentOrganization, Error, Request, selection,
    validate_key_id,
};

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
    let (access_type, bucket_names) = request.into_selection()?;
    let transaction = db.begin().await?;

    // Serialize concurrent changes to this credential before replacing its selection.
    let result = credential::Entity::update_many()
        .set(credential::ActiveModel {
            access_type: Set(access_type),
            ..Default::default()
        })
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .filter(credential::Column::KeyId.eq(path.key_id.clone()))
        .exec(&transaction)
        .await?;
    if result.rows_affected == 0 {
        return Err(Error::new(StatusCode::NOT_FOUND, "credential_not_found"));
    }

    selection::validate(&transaction, organization.id, &bucket_names).await?;
    selection::replace(&transaction, organization.id, &path.key_id, &bucket_names).await?;

    let credential = credential::Entity::find_by_id(path.key_id)
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .one(&transaction)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "credential_not_found"))?;
    transaction.commit().await?;

    Ok(Json(CredentialResponse::new(credential, bucket_names)))
}

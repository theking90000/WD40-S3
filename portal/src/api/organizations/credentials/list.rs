use axum::{Extension, Json, extract::State};
use portal_db::{
    DatabaseConnection, credential,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder},
};

use super::{CredentialResponse, CurrentOrganization, Error};

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
) -> Result<Json<Vec<CredentialResponse>>, Error> {
    let credentials = credential::Entity::find()
        .filter(credential::Column::OrganizationId.eq(organization.id))
        .order_by_asc(credential::Column::KeyId)
        .all(&db)
        .await?;

    Ok(Json(
        credentials
            .into_iter()
            .map(CredentialResponse::from)
            .collect(),
    ))
}

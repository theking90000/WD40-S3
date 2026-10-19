use axum::{Extension, Json, extract::State};
use portal_db::{
    DatabaseConnection, credential, credential_bucket,
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
        .find_with_related(credential_bucket::Entity)
        .all(&db)
        .await?;

    Ok(Json(
        credentials
            .into_iter()
            .map(|(credential, associations)| {
                let mut bucket_names: Vec<_> = associations
                    .into_iter()
                    .map(|association| association.bucket_name)
                    .collect();
                bucket_names.sort_unstable();
                CredentialResponse::new(credential, bucket_names)
            })
            .collect(),
    ))
}

use axum::{Extension, Json, extract::State};
use portal_db::{
    DatabaseConnection, OrganizationMemberAccess, organization, organization_member,
    sea_orm::{
        self, ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, QueryOrder, QuerySelect,
        entity::prelude::{DateTimeWithTimeZone, Uuid},
    },
};

use crate::api::auth::CurrentUser;

use super::{Error, OrganizationResponse, verified_email};

#[derive(FromQueryResult)]
struct OrganizationRow {
    id: Uuid,
    name: String,
    created_at: DateTimeWithTimeZone,
    access: OrganizationMemberAccess,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Json<Vec<OrganizationResponse>>, Error> {
    let email = verified_email(&user)?;
    let organizations = organization_member::Entity::find()
        .inner_join(organization::Entity)
        .filter(organization_member::Column::Email.eq(email))
        .select_only()
        .columns([
            organization::Column::Id,
            organization::Column::Name,
            organization::Column::CreatedAt,
        ])
        .column(organization_member::Column::Access)
        .order_by_desc(organization::Column::CreatedAt)
        .order_by_asc(organization::Column::Id)
        .into_model::<OrganizationRow>()
        .all(&db)
        .await?;

    Ok(Json(
        organizations
            .into_iter()
            .map(|organization| {
                OrganizationResponse::new(
                    organization.id,
                    organization.name,
                    organization.created_at,
                    organization.access,
                )
            })
            .collect(),
    ))
}

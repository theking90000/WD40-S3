use axum::{Extension, Json, extract::State};
use portal_db::{
    DatabaseConnection, organization_member,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder},
};

use super::{CurrentOrganization, Error, MemberResponse};

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
) -> Result<Json<Vec<MemberResponse>>, Error> {
    let members = organization_member::Entity::find()
        .filter(organization_member::Column::OrganizationId.eq(organization.id))
        .order_by_asc(organization_member::Column::Email)
        .all(&db)
        .await?;

    Ok(Json(
        members.into_iter().map(MemberResponse::from).collect(),
    ))
}

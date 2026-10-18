use axum::{
    Extension,
    extract::{Path, State, rejection::PathRejection},
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, OrganizationMemberAccess, organization_member,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter},
};

use serde::Deserialize;

use super::{CurrentOrganization, Error};

#[derive(Deserialize)]
pub(super) struct MemberPath {
    email: String,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    path: Result<Path<MemberPath>, PathRejection>,
) -> Result<StatusCode, Error> {
    organization.require_owner()?;
    let Path(path) = path?;
    let member = organization_member::Entity::find_by_id((organization.id, path.email))
        .one(&db)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "member_not_found"))?;
    if member.access == OrganizationMemberAccess::Owner {
        return Err(Error::new(StatusCode::CONFLICT, "owner_removal_forbidden"));
    }

    let result = organization_member::Entity::delete_many()
        .filter(organization_member::Column::OrganizationId.eq(organization.id))
        .filter(organization_member::Column::Email.eq(member.email))
        .filter(organization_member::Column::Access.eq(OrganizationMemberAccess::Member))
        .exec(&db)
        .await?;
    if result.rows_affected == 0 {
        return Err(Error::new(StatusCode::NOT_FOUND, "member_not_found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

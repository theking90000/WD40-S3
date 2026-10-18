use axum::{
    Extension,
    extract::{Path, State, rejection::PathRejection},
    http::{HeaderMap, StatusCode},
};
use portal_db::{
    DatabaseConnection, OrganizationMemberAccess, organization_member,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter},
};

use crate::api::auth::{CurrentUser, browser_request};

use super::{Error, organization_id, require_owner};

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(user): Extension<CurrentUser>,
    path: Result<Path<(String, String)>, PathRejection>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    browser_request(&headers).map_err(|_| Error::new(StatusCode::FORBIDDEN, "csrf_rejected"))?;
    let Path((id, email)) = path?;
    let id = organization_id(&id)?;
    require_owner(&db, id, &user).await?;
    let member = organization_member::Entity::find_by_id((id, email))
        .one(&db)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "member_not_found"))?;
    if member.access == OrganizationMemberAccess::Owner {
        return Err(Error::new(StatusCode::CONFLICT, "owner_removal_forbidden"));
    }

    let result = organization_member::Entity::delete_many()
        .filter(organization_member::Column::OrganizationId.eq(id))
        .filter(organization_member::Column::Email.eq(member.email))
        .filter(organization_member::Column::Access.eq(OrganizationMemberAccess::Member))
        .exec(&db)
        .await?;
    if result.rows_affected == 0 {
        return Err(Error::new(StatusCode::NOT_FOUND, "member_not_found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

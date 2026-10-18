mod create;
mod delete;
mod list;

use axum::{
    Router,
    http::StatusCode,
    routing::{delete, get},
};
use portal_db::{
    DatabaseConnection, OrganizationMember, OrganizationMemberAccess, organization_member,
    sea_orm::{EntityTrait, entity::prelude::Uuid},
};
use serde::Serialize;

use crate::api::auth::CurrentUser;

use super::{Error, verified_email};

pub(super) fn router() -> Router<DatabaseConnection> {
    Router::new()
        .route(
            "/api/organizations/{id}/members",
            get(list::handle).post(create::handle),
        )
        .route(
            "/api/organizations/{id}/members/{email}",
            delete(delete::handle),
        )
}

#[derive(Serialize)]
pub(super) struct MemberResponse {
    email: String,
    access: &'static str,
    created_at: String,
}

impl From<OrganizationMember> for MemberResponse {
    fn from(member: OrganizationMember) -> Self {
        Self {
            email: member.email,
            access: match member.access {
                OrganizationMemberAccess::Member => "MEMBER",
                OrganizationMemberAccess::Owner => "OWNER",
            },
            created_at: member.created_at.to_rfc3339(),
        }
    }
}

fn organization_id(value: &str) -> Result<Uuid, Error> {
    Uuid::parse_str(value)
        .map_err(|_| Error::new(StatusCode::BAD_REQUEST, "invalid_organization_id"))
}

async fn require_member(
    db: &DatabaseConnection,
    organization_id: Uuid,
    user: &CurrentUser,
) -> Result<OrganizationMemberAccess, Error> {
    let email = verified_email(user)?;
    let member = organization_member::Entity::find_by_id((organization_id, email.to_owned()))
        .one(db)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "organization_not_found"))?;
    Ok(member.access)
}

async fn require_owner(
    db: &DatabaseConnection,
    organization_id: Uuid,
    user: &CurrentUser,
) -> Result<(), Error> {
    match require_member(db, organization_id, user).await? {
        OrganizationMemberAccess::Owner => Ok(()),
        OrganizationMemberAccess::Member => {
            Err(Error::new(StatusCode::FORBIDDEN, "owner_required"))
        }
    }
}

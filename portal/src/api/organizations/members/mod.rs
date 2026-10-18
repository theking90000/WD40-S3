mod create;
mod delete;
mod list;

use axum::{
    Router,
    routing::{delete, get},
};
use portal_db::{DatabaseConnection, OrganizationMember, OrganizationMemberAccess};
use serde::Serialize;

use super::{Error, current::CurrentOrganization};

pub(super) fn router() -> Router<DatabaseConnection> {
    Router::new()
        .route("/members", get(list::handle).post(create::handle))
        .route("/members/{email}", delete(delete::handle))
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

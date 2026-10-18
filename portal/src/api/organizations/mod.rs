mod buckets;
mod create;
mod current;
mod list;
mod members;

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use portal_db::{
    DatabaseConnection, DbErr, OrganizationMemberAccess,
    sea_orm::entity::prelude::{DateTimeWithTimeZone, Uuid},
};
use serde::Serialize;

use super::auth::CurrentUser;

pub(super) fn router(db: DatabaseConnection) -> Router<DatabaseConnection> {
    let organization = members::router()
        .merge(buckets::router())
        .route_layer(axum::middleware::from_fn_with_state(db, current::resolve));

    Router::new()
        .route("/api/organizations", get(list::handle).post(create::handle))
        .nest("/api/organizations/{organization_id}", organization)
        .layer(DefaultBodyLimit::max(16 * 1024))
}

#[derive(Serialize)]
pub(super) struct OrganizationResponse {
    id: String,
    name: String,
    created_at: String,
    access: &'static str,
}

impl OrganizationResponse {
    fn new(
        id: Uuid,
        name: String,
        created_at: DateTimeWithTimeZone,
        access: OrganizationMemberAccess,
    ) -> Self {
        Self {
            id: id.to_string(),
            name,
            created_at: created_at.to_rfc3339(),
            access: match access {
                OrganizationMemberAccess::Member => "MEMBER",
                OrganizationMemberAccess::Owner => "OWNER",
            },
        }
    }
}

fn verified_email(user: &CurrentUser) -> Result<&str, Error> {
    user.email
        .as_deref()
        .filter(|email| !email.trim().is_empty() && user.email_verified == Some(true))
        .ok_or_else(|| Error::new(StatusCode::FORBIDDEN, "email_not_verified"))
}

pub(super) struct Error {
    status: StatusCode,
    code: &'static str,
}

impl Error {
    fn new(status: StatusCode, code: &'static str) -> Self {
        Self { status, code }
    }
}

impl From<DbErr> for Error {
    fn from(error: DbErr) -> Self {
        eprintln!("Organization database operation failed: {error}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "database_error")
    }
}

impl From<JsonRejection> for Error {
    fn from(error: JsonRejection) -> Self {
        match error.status() {
            StatusCode::PAYLOAD_TOO_LARGE => {
                Self::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
            }
            StatusCode::UNSUPPORTED_MEDIA_TYPE => {
                Self::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type")
            }
            _ => Self::new(StatusCode::BAD_REQUEST, "invalid_request"),
        }
    }
}

impl From<PathRejection> for Error {
    fn from(error: PathRejection) -> Self {
        Self::new(error.status(), "invalid_path")
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Body {
            error: &'static str,
        }
        (self.status, Json(Body { error: self.code })).into_response()
    }
}

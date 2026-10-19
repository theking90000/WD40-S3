mod buckets;
mod create;
mod credentials;
mod current;
mod get;
mod list;
mod members;
mod pending;

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
    let organization = Router::new()
        .route("/api/organizations/{organization_id}", get(get::handle))
        .nest(
            "/api/organizations/{organization_id}",
            members::router()
                .merge(buckets::router())
                .merge(credentials::router()),
        )
        .route_layer(axum::middleware::from_fn_with_state(db, current::resolve));

    Router::new()
        .route("/api/organizations", get(list::handle).post(create::handle))
        .merge(organization)
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
    resource_id: Option<String>,
    sync_error: Option<&'static str>,
}

impl Error {
    fn new(status: StatusCode, code: &'static str) -> Self {
        Self {
            status,
            code,
            resource_id: None,
            sync_error: None,
        }
    }

    fn for_resource(mut self, resource_id: &str) -> Self {
        self.resource_id = Some(resource_id.to_owned());
        self
    }

    fn synchronization(error: crate::sync::Error, resource_id: &str) -> Self {
        eprintln!("Synchronization failed for {resource_id}: {}", error.code());
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "synchronization_failed",
            resource_id: Some(resource_id.to_owned()),
            sync_error: Some(error.code()),
        }
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
            #[serde(skip_serializing_if = "Option::is_none")]
            resource_id: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            sync_error: Option<&'static str>,
        }
        (
            self.status,
            Json(Body {
                error: self.code,
                resource_id: self.resource_id,
                sync_error: self.sync_error,
            }),
        )
            .into_response()
    }
}

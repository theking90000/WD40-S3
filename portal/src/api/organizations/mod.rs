mod buckets;
mod create;
mod credentials;
mod current;
mod delete;
mod get;
mod list;
mod members;
mod pending;
mod update;

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
        .route(
            "/api/organizations/{organization_id}",
            get(get::handle).patch(update::handle).delete(delete::handle),
        )
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

fn user_email(user: &CurrentUser) -> Result<&str, Error> {
    let email = user
        .email
        .as_deref()
        .filter(|email| !email.trim().is_empty())
        .ok_or_else(|| Error::new(StatusCode::FORBIDDEN, "email_required"))?;
    #[cfg(feature = "verified-email")]
    if user.email_verified != Some(true) {
        return Err(Error::new(StatusCode::FORBIDDEN, "email_not_verified"));
    }
    Ok(email)
}

fn validate_name(name: &str) -> Result<&str, Error> {
    let name = name.trim();
    if !(1..=128).contains(&name.chars().count()) {
        return Err(Error::new(StatusCode::BAD_REQUEST, "invalid_name"));
    }
    Ok(name)
}

fn name_write_error(error: DbErr) -> Error {
    #[cfg(any(feature = "sqlite", feature = "postgres"))]
    if let DbErr::Exec(portal_db::sea_orm::RuntimeErr::SqlxError(source))
    | DbErr::Query(portal_db::sea_orm::RuntimeErr::SqlxError(source)) = &error
        && let Some(database) = source.as_database_error()
        && database.is_unique_violation()
        && (database.constraint() == Some("idx-organizations-name")
            || database.message() == "UNIQUE constraint failed: organizations.name")
    {
        return Error::new(StatusCode::CONFLICT, "organization_name_taken");
    }
    Error::from(error)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn user(email: Option<&str>, email_verified: Option<bool>) -> CurrentUser {
        CurrentUser {
            issuer: "https://identity.example".to_owned(),
            subject: "alice".to_owned(),
            name: None,
            email: email.map(str::to_owned),
            email_verified,
            roles: Vec::new(),
            groups: Vec::new(),
        }
    }

    #[test]
    fn email_verification_follows_feature() -> Result<(), String> {
        let email = " Alice@example.org ";
        for verified in [Some(true), Some(false), None] {
            match user_email(&user(Some(email), verified)) {
                Ok(value) if (!cfg!(feature = "verified-email") || verified == Some(true))
                    && value == email => {}
                Err(error) if cfg!(feature = "verified-email") && verified != Some(true)
                    && error.status == StatusCode::FORBIDDEN
                    && error.code == "email_not_verified" => {}
                _ => return Err(format!("unexpected email result for verification {verified:?}")),
            }
        }
        Ok(())
    }

    #[test]
    fn missing_email_is_always_rejected() -> Result<(), String> {
        for email in [None, Some(""), Some(" \t\n")] {
            for verified in [Some(true), Some(false), None] {
                match user_email(&user(email, verified)) {
                    Err(error) if error.status == StatusCode::FORBIDDEN
                        && error.code == "email_required" => {}
                    _ => return Err(format!("unexpected result for email {email:?}")),
                }
            }
        }
        Ok(())
    }
}

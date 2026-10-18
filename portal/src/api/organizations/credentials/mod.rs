mod create;
mod delete;
mod list;
mod update;

use axum::{
    Router,
    http::StatusCode,
    routing::{delete, get},
};
use portal_db::{Credential, CredentialAccessType, DatabaseConnection};
use serde::{Deserialize, Serialize};

use super::{Error, current::CurrentOrganization};

pub(super) fn router() -> Router<DatabaseConnection> {
    Router::new()
        .route("/credentials", get(list::handle).post(create::handle))
        .route(
            "/credentials/{key_id}",
            delete(delete::handle).patch(update::handle),
        )
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum AccessType {
    All,
    Restricted,
}

impl From<AccessType> for CredentialAccessType {
    fn from(access_type: AccessType) -> Self {
        match access_type {
            AccessType::All => Self::All,
            AccessType::Restricted => Self::Restricted,
        }
    }
}

impl From<CredentialAccessType> for AccessType {
    fn from(access_type: CredentialAccessType) -> Self {
        match access_type {
            CredentialAccessType::All => Self::All,
            CredentialAccessType::Restricted => Self::Restricted,
        }
    }
}

#[derive(Serialize)]
pub(super) struct CredentialResponse {
    organization_id: String,
    key_id: String,
    access_type: AccessType,
    created_at: String,
}

impl From<Credential> for CredentialResponse {
    fn from(credential: Credential) -> Self {
        Self {
            organization_id: credential.organization_id.to_string(),
            key_id: credential.key_id,
            access_type: credential.access_type.into(),
            created_at: credential.created_at.to_rfc3339(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct CredentialPath {
    key_id: String,
}

fn validate_key_id(key_id: &str) -> Result<(), Error> {
    if key_id.len() != 20 || !key_id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(Error::new(StatusCode::BAD_REQUEST, "invalid_key_id"));
    }
    Ok(())
}

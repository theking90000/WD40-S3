mod create;
mod delete;
mod list;
mod selection;
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

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum AccessType {
    All,
    Restricted,
}

impl From<CredentialAccessType> for AccessType {
    fn from(access_type: CredentialAccessType) -> Self {
        match access_type {
            CredentialAccessType::All => Self::All,
            CredentialAccessType::Restricted => Self::Restricted,
        }
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "access_type",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub(super) enum Request {
    All {},
    Restricted { bucket_names: Vec<String> },
}

impl Request {
    fn into_selection(self) -> Result<(CredentialAccessType, Vec<String>), Error> {
        match self {
            Self::All {} => Ok((CredentialAccessType::All, Vec::new())),
            Self::Restricted { mut bucket_names } => {
                bucket_names.sort_unstable();
                let count = bucket_names.len();
                bucket_names.dedup();
                if bucket_names.len() != count {
                    return Err(Error::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_bucket_selection",
                    ));
                }
                Ok((CredentialAccessType::Restricted, bucket_names))
            }
        }
    }
}

#[derive(Serialize)]
pub(super) struct CredentialResponse {
    organization_id: String,
    key_id: String,
    access_type: AccessType,
    created_at: String,
    bucket_names: Vec<String>,
}

impl CredentialResponse {
    fn new(credential: Credential, bucket_names: Vec<String>) -> Self {
        Self {
            organization_id: credential.organization_id.to_string(),
            key_id: credential.key_id,
            access_type: credential.access_type.into(),
            created_at: credential.created_at.to_rfc3339(),
            bucket_names: match credential.access_type {
                CredentialAccessType::All => Vec::new(),
                CredentialAccessType::Restricted => bucket_names,
            },
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

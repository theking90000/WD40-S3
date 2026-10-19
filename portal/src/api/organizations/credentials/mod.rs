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
use serde::{Deserialize, Deserializer, Serialize};

use super::{Error, current::CurrentOrganization};

pub(super) fn router() -> Router<DatabaseConnection> {
    Router::new()
        .route("/credentials", get(list::handle).post(create::handle))
        .route(
            "/credentials/{key_id}",
            delete(delete::handle).patch(update::handle),
        )
}

#[derive(Clone, Copy, Deserialize, Serialize)]
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

impl From<AccessType> for CredentialAccessType {
    fn from(access_type: AccessType) -> Self {
        match access_type {
            AccessType::All => Self::All,
            AccessType::Restricted => Self::Restricted,
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Permission {
    Read,
    Write,
    List,
    Delete,
}

impl Permission {
    fn bit(self) -> i16 {
        match self {
            Self::Read => 1,
            Self::Write => 2,
            Self::List => 8,
            Self::Delete => 4,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    #[serde(default, deserialize_with = "provided")]
    access_type: Option<AccessType>,
    #[serde(default, deserialize_with = "provided")]
    bucket_names: Option<Vec<String>>,
    #[serde(default, deserialize_with = "provided")]
    permissions: Option<Vec<Permission>>,
}

impl Request {
    fn into_selection(
        self,
        current: Option<&Credential>,
        existing_bucket_names: Vec<String>,
    ) -> Result<(CredentialAccessType, Vec<String>, i16), Error> {
        if self.access_type.is_none() && self.bucket_names.is_none() && self.permissions.is_none() {
            return Err(Error::new(StatusCode::BAD_REQUEST, "invalid_request"));
        }
        let access_type = self
            .access_type
            .map(CredentialAccessType::from)
            .or_else(|| current.map(|credential| credential.access_type))
            .ok_or_else(|| Error::new(StatusCode::BAD_REQUEST, "invalid_request"))?;
        let mut bucket_names = match access_type {
            CredentialAccessType::All => {
                if self.bucket_names.is_some() {
                    return Err(Error::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_bucket_selection",
                    ));
                }
                Vec::new()
            }
            CredentialAccessType::Restricted => self
                .bucket_names
                .or_else(|| {
                    current
                        .filter(|credential| {
                            credential.access_type == CredentialAccessType::Restricted
                        })
                        .map(|_| existing_bucket_names)
                })
                .ok_or_else(|| Error::new(StatusCode::BAD_REQUEST, "invalid_bucket_selection"))?,
        };
        bucket_names.sort_unstable();
        let count = bucket_names.len();
        bucket_names.dedup();
        if bucket_names.len() != count {
            return Err(Error::new(
                StatusCode::BAD_REQUEST,
                "invalid_bucket_selection",
            ));
        }

        let permissions = match self.permissions {
            Some(permissions) => {
                let mut mask = 0;
                for permission in permissions {
                    let bit = permission.bit();
                    if mask & bit != 0 {
                        return Err(Error::new(StatusCode::BAD_REQUEST, "invalid_permissions"));
                    }
                    mask |= bit;
                }
                mask
            }
            None => current.map_or(portal_db::credential::ALL_PERMISSIONS, |credential| {
                credential.permissions
            }),
        };
        Ok((access_type, bucket_names, permissions))
    }
}

// Missing fields are optional; an explicitly supplied null is never a default.
fn provided<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Serialize)]
pub(super) struct CredentialResponse {
    organization_id: String,
    key_id: String,
    access_type: AccessType,
    created_at: String,
    bucket_names: Vec<String>,
    permissions: Vec<Permission>,
}

impl CredentialResponse {
    fn new(credential: Credential, bucket_names: Vec<String>) -> Self {
        let permissions = [
            Permission::Read,
            Permission::Write,
            Permission::List,
            Permission::Delete,
        ]
        .into_iter()
        .filter(|permission| credential.permissions & permission.bit() != 0)
        .collect();
        Self {
            organization_id: credential.organization_id.to_string(),
            key_id: credential.key_id,
            access_type: credential.access_type.into(),
            created_at: credential.created_at.to_rfc3339(),
            permissions,
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

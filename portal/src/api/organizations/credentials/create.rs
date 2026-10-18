use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use portal_db::{
    CredentialAccessType, DatabaseConnection, credential,
    sea_orm::{ActiveModelTrait, Set, SqlErr},
};
use serde::{Deserialize, Serialize};

use super::{AccessType, CredentialResponse, CurrentOrganization, Error};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    access_type: AccessType,
}

// The secret exists only in the creation response, never in the persisted model.
#[derive(Serialize)]
pub(super) struct Response {
    #[serde(flatten)]
    credential: CredentialResponse,
    secret: String,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<(StatusCode, Json<Response>), Error> {
    organization.require_owner()?;
    let Json(request) = payload?;
    let access_type: CredentialAccessType = request.access_type.into();
    let secret = random_key(40)?;

    for _ in 0..3 {
        let key_id = random_key(20)?;
        let result = credential::ActiveModel {
            key_id: Set(key_id),
            organization_id: Set(organization.id),
            access_type: Set(access_type),
            ..Default::default()
        }
        .insert(&db)
        .await;

        match result {
            Ok(credential) => {
                return Ok((
                    StatusCode::CREATED,
                    Json(Response {
                        credential: CredentialResponse::from(credential),
                        secret,
                    }),
                ));
            }
            Err(error) => match error.sql_err() {
                Some(SqlErr::UniqueConstraintViolation(_)) => continue,
                _ => return Err(Error::from(error)),
            },
        }
    }

    Err(Error::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "key_generation_failed",
    ))
}

fn random_key(length: usize) -> Result<String, Error> {
    let mut value = String::with_capacity(length);
    let mut bytes = [0; 64];
    while value.len() < length {
        getrandom::fill(&mut bytes)
            .map_err(|_| Error::new(StatusCode::INTERNAL_SERVER_ERROR, "key_generation_failed"))?;
        // Reject other bytes to sample the ASCII alphanumeric alphabet uniformly.
        for byte in bytes {
            if byte.is_ascii_alphanumeric() {
                value.push(char::from(byte));
                if value.len() == length {
                    break;
                }
            }
        }
    }
    Ok(value)
}

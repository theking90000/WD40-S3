use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, credential, credential_sync,
    sea_orm::{ActiveModelTrait, EntityTrait, Set, SqlErr, TransactionTrait},
};
use s3_credentials_registry::KeySecret;
use serde::Serialize;

use super::super::pending;
use super::{CredentialResponse, CurrentOrganization, Error, Request, selection};
use crate::sync::{self, SyncContext};

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
    Extension(sync): Extension<SyncContext>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<(StatusCode, Json<Response>), Error> {
    organization.require_owner()?;
    let Json(request) = payload?;
    let (access_type, bucket_names) = request.into_selection()?;

    for _ in 0..3 {
        let key_id = random_key(20)?;
        let transaction = db.begin().await?;
        selection::validate(&transaction, organization.id, &bucket_names).await?;
        if credential_sync::Entity::find_by_id(key_id.clone())
            .one(&transaction)
            .await?
            .is_some()
        {
            transaction.rollback().await?;
            continue;
        }
        let result = credential::ActiveModel {
            key_id: Set(key_id),
            organization_id: Set(organization.id),
            access_type: Set(access_type),
            ..Default::default()
        }
        .insert(&transaction)
        .await;

        match result {
            Ok(credential) => {
                selection::replace(
                    &transaction,
                    organization.id,
                    &credential.key_id,
                    &bucket_names,
                )
                .await?;
                pending::selected_buckets(
                    &transaction,
                    organization.id,
                    access_type,
                    &bucket_names,
                )
                .await?;
                let state = pending::ensure_credential(&transaction, &credential.key_id).await?;
                transaction.commit().await?;

                let secret =
                    random_key(40).map_err(|error| error.for_resource(&credential.key_id))?;
                let signing_secret = KeySecret::new(secret.clone())
                    .map_err(|error| Error::synchronization(error.into(), &credential.key_id))?;
                let applied = sync::register_credential(&sync, &credential.key_id, signing_secret)
                    .await
                    .map_err(|error| Error::synchronization(error, &credential.key_id))?;
                pending::require_applied(applied, state.generation, &credential.key_id)?;
                return Ok((
                    StatusCode::CREATED,
                    Json(Response {
                        credential: CredentialResponse::new(credential, bucket_names),
                        secret,
                    }),
                ));
            }
            Err(error) => {
                // PostgreSQL aborts the transaction on a collision; retry a fresh one.
                transaction.rollback().await?;
                match error.sql_err() {
                    Some(SqlErr::UniqueConstraintViolation(_)) => continue,
                    _ => return Err(Error::from(error)),
                }
            }
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

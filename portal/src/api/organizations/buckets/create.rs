use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, bucket,
    sea_orm::{ActiveModelTrait, Set, SqlErr},
};
use serde::Deserialize;

use super::{Access, BucketResponse, CurrentOrganization, Error};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    name: String,
    access: Access,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<(StatusCode, Json<BucketResponse>), Error> {
    organization.require_owner()?;
    let Json(request) = payload?;
    validate_name(&request.name)?;

    let bucket = bucket::ActiveModel {
        name: Set(request.name),
        organization_id: Set(organization.id),
        access: Set(request.access.into()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .map_err(|error| match error.sql_err() {
        Some(SqlErr::UniqueConstraintViolation(_)) => {
            Error::new(StatusCode::CONFLICT, "bucket_already_exists")
        }
        _ => Error::from(error),
    })?;

    Ok((StatusCode::CREATED, Json(BucketResponse::from(bucket))))
}

fn validate_name(name: &str) -> Result<(), Error> {
    let bytes = name.as_bytes();
    let alphanumeric = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let ip_address = name.split('.').count() == 4
        && name.split('.').all(|part| part.parse::<u8>().is_ok());
    let reserved_prefix = ["xn--", "sthree-", "amzn-s3-demo-"]
        .into_iter()
        .any(|prefix| name.starts_with(prefix));
    // Account-regional AWS namespaces are not represented by the portal.
    let reserved_suffix = ["-s3alias", "--ol-s3", ".mrap", "--x-s3", "--table-s3", "-an"]
        .into_iter()
        .any(|suffix| name.ends_with(suffix));

    if !(3..=63).contains(&bytes.len())
        || !bytes.first().is_some_and(alphanumeric)
        || !bytes.last().is_some_and(alphanumeric)
        || bytes
            .iter()
            .any(|byte| !alphanumeric(byte) && *byte != b'.' && *byte != b'-')
        || name.contains("..")
        || ip_address
        || reserved_prefix
        || reserved_suffix
    {
        return Err(Error::new(StatusCode::BAD_REQUEST, "invalid_bucket_name"));
    }

    Ok(())
}

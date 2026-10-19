use axum::http::StatusCode;
use portal_db::{
    DbErr, bucket, credential_bucket,
    sea_orm::{
        ColumnTrait, DatabaseTransaction, EntityTrait, PaginatorTrait, QueryFilter, Set, SqlErr,
        entity::prelude::Uuid,
    },
};

use super::Error;

pub(super) async fn validate(
    transaction: &DatabaseTransaction,
    organization_id: Uuid,
    bucket_names: &[String],
) -> Result<(), Error> {
    if bucket_names.is_empty() {
        return Ok(());
    }

    let count = bucket::Entity::find()
        .filter(bucket::Column::OrganizationId.eq(organization_id))
        .filter(bucket::Column::Name.is_in(bucket_names.iter().cloned()))
        .count(transaction)
        .await?;
    if usize::try_from(count).ok() != Some(bucket_names.len()) {
        return Err(Error::new(
            StatusCode::BAD_REQUEST,
            "invalid_bucket_selection",
        ));
    }
    Ok(())
}

pub(super) async fn replace(
    transaction: &DatabaseTransaction,
    organization_id: Uuid,
    key_id: &str,
    bucket_names: &[String],
) -> Result<(), Error> {
    credential_bucket::Entity::delete_many()
        .filter(credential_bucket::Column::OrganizationId.eq(organization_id))
        .filter(credential_bucket::Column::KeyId.eq(key_id))
        .exec(transaction)
        .await?;

    if !bucket_names.is_empty() {
        credential_bucket::Entity::insert_many(bucket_names.iter().map(|name| {
            credential_bucket::ActiveModel {
                organization_id: Set(organization_id),
                key_id: Set(key_id.to_owned()),
                bucket_name: Set(name.to_owned()),
            }
        }))
        .exec_without_returning(transaction)
        .await
        .map_err(insert_error)?;
    }
    Ok(())
}

fn insert_error(error: DbErr) -> Error {
    match error.sql_err() {
        Some(SqlErr::ForeignKeyConstraintViolation(_)) => {
            Error::new(StatusCode::BAD_REQUEST, "invalid_bucket_selection")
        }
        _ => Error::from(error),
    }
}

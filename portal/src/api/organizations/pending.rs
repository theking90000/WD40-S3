//! SQL preparation shared by organization writes; performs no registry calls.

use axum::http::StatusCode;
use portal_db::{
    BucketSync, CredentialAccessType, CredentialSync, bucket, bucket_sync, credential,
    credential_sync,
    sea_orm::{
        ActiveModelBehavior, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter,
        QueryOrder, Set, entity::prelude::Uuid, sea_query::OnConflict,
    },
};

use super::Error;

pub(super) async fn ensure_bucket(
    transaction: &DatabaseTransaction,
    name: &str,
) -> Result<BucketSync, Error> {
    if let Some(state) = bucket_sync::Entity::find_by_id(name.to_owned())
        .one(transaction)
        .await?
    {
        return Ok(state);
    }
    let model = bucket_sync::ActiveModel {
        bucket_name: Set(name.to_owned()),
        ..Default::default()
    }
    .before_save(transaction, true)
    .await?;
    // Entity inserts do not call before_save; generate IDs before the upsert.
    bucket_sync::Entity::insert(model)
        .on_conflict(
            OnConflict::column(bucket_sync::Column::BucketName)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(transaction)
        .await?;
    bucket_sync::Entity::find_by_id(name.to_owned())
        .one(transaction)
        .await?
        .ok_or_else(|| Error::new(StatusCode::INTERNAL_SERVER_ERROR, "sync_state_missing"))
}

pub(super) async fn ensure_credential(
    transaction: &DatabaseTransaction,
    key_id: &str,
) -> Result<CredentialSync, Error> {
    if let Some(state) = credential_sync::Entity::find_by_id(key_id.to_owned())
        .one(transaction)
        .await?
    {
        return Ok(state);
    }
    let model = credential_sync::ActiveModel {
        key_id: Set(key_id.to_owned()),
        ..Default::default()
    }
    .before_save(transaction, true)
    .await?;
    credential_sync::Entity::insert(model)
        .on_conflict(
            OnConflict::column(credential_sync::Column::KeyId)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(transaction)
        .await?;
    credential_sync::Entity::find_by_id(key_id.to_owned())
        .one(transaction)
        .await?
        .ok_or_else(|| Error::new(StatusCode::INTERNAL_SERVER_ERROR, "sync_state_missing"))
}

pub(super) async fn advance_bucket(
    transaction: &DatabaseTransaction,
    name: &str,
) -> Result<BucketSync, Error> {
    let mut state = ensure_bucket(transaction, name).await?;
    if state.delete_requested {
        return Err(Error::new(StatusCode::CONFLICT, "resource_deleting"));
    }
    let generation = next_generation(state.generation)?;
    let result = bucket_sync::Entity::update_many()
        .set(bucket_sync::ActiveModel {
            generation: Set(generation),
            last_error: Set(None),
            ..Default::default()
        })
        .filter(bucket_sync::Column::BucketName.eq(name.to_owned()))
        .filter(bucket_sync::Column::Generation.eq(state.generation))
        .filter(bucket_sync::Column::DeleteRequested.eq(false))
        .exec(transaction)
        .await?;
    if result.rows_affected != 1 {
        return Err(Error::new(StatusCode::CONFLICT, "sync_state_changed"));
    }
    state.generation = generation;
    state.last_error = None;
    Ok(state)
}

pub(super) async fn advance_credential(
    transaction: &DatabaseTransaction,
    key_id: &str,
) -> Result<i64, Error> {
    let state = ensure_credential(transaction, key_id).await?;
    if state.delete_requested {
        return Err(Error::new(StatusCode::CONFLICT, "resource_deleting"));
    }
    let generation = next_generation(state.generation)?;
    let result = credential_sync::Entity::update_many()
        .set(credential_sync::ActiveModel {
            generation: Set(generation),
            last_error: Set(None),
            ..Default::default()
        })
        .filter(credential_sync::Column::KeyId.eq(key_id.to_owned()))
        .filter(credential_sync::Column::Generation.eq(state.generation))
        .filter(credential_sync::Column::DeleteRequested.eq(false))
        .exec(transaction)
        .await?;
    if result.rows_affected != 1 {
        return Err(Error::new(StatusCode::CONFLICT, "sync_state_changed"));
    }
    Ok(generation)
}

pub(super) async fn delete_credential(
    transaction: &DatabaseTransaction,
    key_id: &str,
) -> Result<(), Error> {
    let state = ensure_credential(transaction, key_id).await?;
    if state.delete_requested {
        return Ok(());
    }
    let generation = next_generation(state.generation)?;
    let result = credential_sync::Entity::update_many()
        .set(credential_sync::ActiveModel {
            generation: Set(generation),
            delete_requested: Set(true),
            last_error: Set(None),
            ..Default::default()
        })
        .filter(credential_sync::Column::KeyId.eq(key_id.to_owned()))
        .filter(credential_sync::Column::PrincipalId.eq(state.principal_id))
        .filter(credential_sync::Column::Generation.eq(state.generation))
        .filter(credential_sync::Column::DeleteRequested.eq(false))
        .exec(transaction)
        .await?;
    if result.rows_affected != 1 {
        return Err(Error::new(StatusCode::CONFLICT, "sync_state_changed"));
    }
    Ok(())
}

pub(super) async fn selected_buckets(
    transaction: &DatabaseTransaction,
    organization_id: Uuid,
    access_type: CredentialAccessType,
    names: &[String],
) -> Result<(), Error> {
    let mut query =
        bucket::Entity::find().filter(bucket::Column::OrganizationId.eq(organization_id));
    if access_type == CredentialAccessType::Restricted {
        query = query.filter(bucket::Column::Name.is_in(names.iter().cloned()));
    }
    for bucket in query
        .order_by_asc(bucket::Column::Name)
        .all(transaction)
        .await?
    {
        let state = ensure_bucket(transaction, &bucket.name).await?;
        if state.delete_requested && access_type == CredentialAccessType::Restricted {
            return Err(Error::new(StatusCode::CONFLICT, "resource_deleting"));
        }
    }
    Ok(())
}

pub(super) async fn all_credentials(
    transaction: &DatabaseTransaction,
    organization_id: Uuid,
) -> Result<(), Error> {
    let credentials = credential::Entity::find()
        .filter(credential::Column::OrganizationId.eq(organization_id))
        .filter(credential::Column::AccessType.eq(CredentialAccessType::All))
        .order_by_asc(credential::Column::KeyId)
        .all(transaction)
        .await?;
    if credentials.is_empty() {
        return Ok(());
    }
    selected_buckets(transaction, organization_id, CredentialAccessType::All, &[]).await?;
    for credential in credentials {
        let state = ensure_credential(transaction, &credential.key_id).await?;
        if !state.delete_requested {
            advance_credential(transaction, &credential.key_id).await?;
        }
    }
    Ok(())
}

pub(super) fn require_applied(
    applied: crate::sync::Applied,
    generation: i64,
    resource_id: &str,
) -> Result<(), Error> {
    if applied.generation != generation {
        return Err(Error::synchronization(
            crate::sync::Error::StateChanged,
            resource_id,
        ));
    }
    Ok(())
}

fn next_generation(generation: i64) -> Result<i64, Error> {
    generation.checked_add(1).ok_or_else(|| {
        Error::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "sync_generation_overflow",
        )
    })
}

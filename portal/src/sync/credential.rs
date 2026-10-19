use std::collections::{BTreeMap, BTreeSet};

use policy_registry::{Actions, Effect, Policy, PrincipalSelector, ResourcePattern, Statement, traits::PolicyRegistry as _};
use portal_db::{
    Bucket, BucketSync, CredentialAccessType, CredentialSync, bucket, bucket_sync, credential,
    credential_bucket, credential_sync, legacy_bucket_policy,
    sea_orm::{
        ColumnTrait, EntityTrait, IsolationLevel, QueryFilter, QueryOrder, Set,
        TransactionTrait, entity::prelude::Uuid,
    },
};
use s3_credentials_registry::{KeyId, KeySecret, traits::S3CredentialRegistry as _};

use super::{Applied, Error, SyncContext};

/// Reconciles an existing WD40 key and its bindings without creating a secret.
pub async fn sync_credential(context: &SyncContext, key_id: &str) -> Result<Applied, Error> {
    let _permit = context.acquire().await?;
    synchronize(context, key_id, None).await
}

/// Prepares bindings and registers a key using the caller's in-memory secret.
/// The secret is never written to SQL or synchronization diagnostics.
pub async fn register_credential(
    context: &SyncContext,
    key_id: &str,
    secret: KeySecret,
) -> Result<Applied, Error> {
    let _permit = context.acquire().await?;
    synchronize(context, key_id, Some(secret)).await
}

pub(super) async fn synchronize(
    context: &SyncContext,
    key_id: &str,
    secret: Option<KeySecret>,
) -> Result<Applied, Error> {
    let state = credential_sync::Entity::find_by_id(key_id.to_owned())
        .one(&context.db)
        .await?
        .ok_or(Error::MissingState("credential"))?;
    if state.delete_requested {
        return Err(Error::DeletionRequested);
    }
    record(context, &state, Some("sync_incomplete"), false).await?;
    let result = apply(context, &state, secret).await;
    record(
        context,
        &state,
        result.as_ref().err().map(Error::code),
        result.is_ok(),
    )
    .await?;
    result?;
    Ok(Applied {
        generation: state.generation,
    })
}

async fn apply(
    context: &SyncContext,
    state: &CredentialSync,
    secret: Option<KeySecret>,
) -> Result<(), Error> {
    let key_id = KeyId::new(state.key_id.clone())?;
    let (buckets, actions) = selected_buckets(context, state).await?;
    let existing = context
        .call("get credential", context.credentials.get_key(&key_id))
        .await?;
    if let Some(existing) = &existing {
        if existing.principal_id != state.principal_id.as_u128() {
            return Err(Error::PrincipalMismatch);
        }
        if secret
            .as_ref()
            .is_some_and(|secret| secret != &existing.secret)
        {
            return Err(s3_credentials_registry::Error::KeyAlreadyExists(key_id).into());
        }
    } else if secret.is_none() {
        return Err(Error::CredentialMissing);
    }

    let current = bindings(context, state.principal_id.as_u128()).await?;
    let managed = managed_policies(context, &current).await?;
    let selector = PrincipalSelector::Specific(state.principal_id.as_u128());
    // Legacy ALL grants must be gone before a restricted policy is confirmed.
    for id in current.intersection(&managed) {
        context
            .call(
                "detach private policy",
                context.policies.detach(selector, *id),
            )
            .await?;
    }
    let policy = Policy {
        statements: if actions.is_empty() {
            Vec::new()
        } else {
            buckets.iter().map(|(bucket, _)| Statement {
                effect: Effect::Allow,
                actions,
                resource: ResourcePattern::Prefix(format!("{}/", bucket.name)),
            }).collect()
        },
    };
    super::put_policy(context, state.policy_id.as_u128(), policy).await?;
    context.call(
        "attach credential policy",
        context.policies.attach(selector, state.policy_id.as_u128()),
    ).await?;
    if existing.is_none() {
        let secret = secret.ok_or(Error::CredentialMissing)?;
        context
            .call(
                "register credential",
                context
                    .credentials
                    .register_key(key_id, secret, state.principal_id.as_u128()),
            )
            .await?;
    }
    Ok(())
}

async fn selected_buckets(
    context: &SyncContext,
    state: &CredentialSync,
) -> Result<(Vec<(Bucket, BucketSync)>, Actions), Error> {
    let transaction = context
        .db
        .begin_with_config(Some(IsolationLevel::Serializable), None)
        .await?;
    let current = credential_sync::Entity::find_by_id(state.key_id.clone())
        .one(&transaction)
        .await?
        .ok_or(Error::StateChanged)?;
    if current.generation != state.generation
        || current.principal_id != state.principal_id
        || current.policy_id != state.policy_id
        || current.delete_requested
    {
        return Err(Error::StateChanged);
    }
    let credential = credential::Entity::find_by_id(state.key_id.clone())
        .one(&transaction)
        .await?
        .ok_or(Error::CredentialNotFound)?;
    if !(0..=credential::ALL_PERMISSIONS).contains(&credential.permissions) {
        return Err(Error::InvalidPermissions);
    }
    let mut actions = Actions::EMPTY;
    for (bit, action) in [(1, Actions::READ), (2, Actions::WRITE), (4, Actions::DELETE), (8, Actions::LIST)] {
        if credential.permissions & bit != 0 {
            actions |= action;
        }
    }
    let mut query = bucket::Entity::find()
        .filter(bucket::Column::OrganizationId.eq(credential.organization_id));
    if credential.access_type == CredentialAccessType::Restricted {
        let selection = credential_bucket::Entity::find()
            .filter(credential_bucket::Column::OrganizationId.eq(credential.organization_id))
            .filter(credential_bucket::Column::KeyId.eq(state.key_id.clone()))
            .all(&transaction)
            .await?;
        query = query
            .filter(bucket::Column::Name.is_in(selection.into_iter().map(|row| row.bucket_name)));
    }
    let buckets = query
        .order_by_asc(bucket::Column::Name)
        .all(&transaction)
        .await?;
    let mut states: BTreeMap<_, _> = bucket_sync::Entity::find()
        .filter(
            bucket_sync::Column::BucketName.is_in(buckets.iter().map(|bucket| bucket.name.clone())),
        )
        .all(&transaction)
        .await?
        .into_iter()
        .map(|state| (state.bucket_name.clone(), state))
        .collect();
    let mut selected = Vec::new();
    for bucket in buckets {
        let state = states
            .remove(&bucket.name)
            .ok_or(Error::MissingState("bucket"))?;
        if !state.delete_requested {
            selected.push((bucket, state));
        }
    }
    transaction.commit().await?;
    Ok((selected, actions))
}

async fn bindings(context: &SyncContext, principal: u128) -> Result<BTreeSet<u128>, Error> {
    let selectors = [PrincipalSelector::Specific(principal)];
    let snapshot = context
        .call(
            "load credential policies",
            context.policies.load_policies(&selectors),
        )
        .await?;
    Ok(snapshot.policies.into_iter().map(|(id, _)| id).collect())
}

async fn managed_policies(
    context: &SyncContext,
    current: &BTreeSet<u128>,
) -> Result<BTreeSet<u128>, Error> {
    if current.is_empty() {
        return Ok(BTreeSet::new());
    }
    let public = bucket_sync::Entity::find()
        .filter(bucket_sync::Column::PublicPolicyId.is_in(current.iter().copied().map(Uuid::from_u128)))
        .all(&context.db)
        .await?;
    let legacy = legacy_bucket_policy::Entity::find()
        .filter(legacy_bucket_policy::Column::PolicyId.is_in(current.iter().copied().map(Uuid::from_u128)))
        .all(&context.db)
        .await?;
    Ok(public.into_iter().map(|state| state.public_policy_id.as_u128())
        .chain(legacy.into_iter().map(|policy| policy.policy_id.as_u128()))
        .collect())
}

/// Requires a persisted deletion request. Removes SQL metadata after WD40 cleanup.
pub async fn delete_credential(context: &SyncContext, key_id: &str) -> Result<(), Error> {
    let _permit = context.acquire().await?;
    delete_requested(context, key_id).await
}

pub(super) async fn delete_requested(context: &SyncContext, key_id: &str) -> Result<(), Error> {
    let state = credential_sync::Entity::find_by_id(key_id.to_owned())
        .one(&context.db)
        .await?;
    let Some(state) = state else {
        if credential::Entity::find_by_id(key_id.to_owned())
            .one(&context.db)
            .await?
            .is_none()
        {
            return Ok(());
        }
        return Err(Error::MissingState("credential"));
    };
    if !state.delete_requested {
        return Err(Error::DeletionNotRequested);
    }
    record(context, &state, Some("sync_incomplete"), false).await?;
    let result = delete(context, &state).await;
    if let Err(error) = &result {
        record(context, &state, Some(error.code()), false).await?;
    }
    result
}

async fn delete(context: &SyncContext, state: &CredentialSync) -> Result<(), Error> {
    let key_id = KeyId::new(state.key_id.clone())?;
    let existing = context
        .call("get credential", context.credentials.get_key(&key_id))
        .await?;
    if existing.is_some_and(|credential| credential.principal_id != state.principal_id.as_u128()) {
        return Err(Error::PrincipalMismatch);
    }
    context
        .call("revoke credential", context.credentials.revoke_key(&key_id))
        .await?;
    let current = bindings(context, state.principal_id.as_u128()).await?;
    let managed = managed_policies(context, &current).await?;
    for id in current.intersection(&managed) {
        context
            .call(
                "detach private policy",
                context.policies.detach(
                    PrincipalSelector::Specific(state.principal_id.as_u128()),
                    *id,
                ),
            )
            .await?;
    }
    context.call(
        "delete credential policy",
        context.policies.delete_policy(state.policy_id.as_u128()),
    ).await?;

    let transaction = context.db.begin().await?;
    let result = credential_sync::Entity::delete_many()
        .filter(credential_sync::Column::KeyId.eq(state.key_id.clone()))
        .filter(credential_sync::Column::Generation.eq(state.generation))
        .filter(credential_sync::Column::PrincipalId.eq(state.principal_id))
        .filter(credential_sync::Column::PolicyId.eq(state.policy_id))
        .filter(credential_sync::Column::DeleteRequested.eq(true))
        .exec(&transaction)
        .await?;
    if result.rows_affected != 1 {
        return Err(Error::StateChanged);
    }
    credential::Entity::delete_by_id(state.key_id.clone())
        .exec(&transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

async fn record(
    context: &SyncContext,
    state: &CredentialSync,
    error: Option<&str>,
    applied: bool,
) -> Result<(), Error> {
    let mut update = credential_sync::ActiveModel {
        last_error: Set(error.map(str::to_owned)),
        ..Default::default()
    };
    if applied {
        update.applied_generation = Set(state.generation);
    }
    let result = credential_sync::Entity::update_many()
        .set(update)
        .filter(credential_sync::Column::KeyId.eq(state.key_id.clone()))
        .filter(credential_sync::Column::Generation.eq(state.generation))
        .filter(credential_sync::Column::PrincipalId.eq(state.principal_id))
        .filter(credential_sync::Column::PolicyId.eq(state.policy_id))
        .filter(credential_sync::Column::DeleteRequested.eq(state.delete_requested))
        .exec(&context.db)
        .await?;
    if result.rows_affected != 1 {
        return Err(Error::StateChanged);
    }
    Ok(())
}

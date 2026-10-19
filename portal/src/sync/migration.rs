use std::collections::BTreeSet;

use policy_registry::traits::PolicyRegistry as _;
use portal_db::{
    bucket_sync, credential, credential_sync, legacy_bucket_policy,
    sea_orm::{EntityTrait, QueryOrder},
};

use super::{Error, SyncContext};

/// Explicit, resumable transition to per-key policies. Run with other portal
/// processes stopped, using the same SQL database and WD40 registries.
/// This never generates or changes a key's secret or principal.
pub async fn migrate_credential_policies(context: &SyncContext) -> Result<(), Error> {
    let _permit = context.acquire().await?;
    let states = credential_sync::Entity::find()
        .order_by_asc(credential_sync::Column::KeyId)
        .all(&context.db)
        .await?;
    let tracked: BTreeSet<_> = states.iter().map(|state| state.key_id.as_str()).collect();
    for credential in credential::Entity::find().all(&context.db).await? {
        if !tracked.contains(credential.key_id.as_str()) {
            eprintln!(
                "Credential policy migration has no sync state for {}",
                credential.key_id
            );
            return Err(Error::MissingState("credential"));
        }
    }
    for state in &states {
        let result = if state.delete_requested {
            super::credential::delete_requested(context, &state.key_id).await
        } else {
            super::credential::synchronize(context, &state.key_id, None)
                .await
                .map(|_| ())
        };
        result.map_err(|error| {
            eprintln!(
                "Credential policy migration failed for {}: {}",
                state.key_id,
                error.code()
            );
            error
        })?;
    }

    for bucket in bucket_sync::Entity::find()
        .order_by_asc(bucket_sync::Column::BucketName)
        .all(&context.db)
        .await?
    {
        super::bucket::synchronize(context, &bucket.bucket_name)
            .await
            .map_err(|error| {
                eprintln!(
                    "Public policy migration failed for {}: {}",
                    bucket.bucket_name,
                    error.code()
                );
                error
            })?;
    }

    // Do not delete shared grants until every tracked key has been converted.
    for state in credential_sync::Entity::find().all(&context.db).await? {
        if state.delete_requested
            || state.applied_generation != state.generation
            || state.last_error.is_some()
        {
            return Err(Error::MigrationPending);
        }
    }
    for policy in legacy_bucket_policy::Entity::find()
        .order_by_asc(legacy_bucket_policy::Column::PolicyId)
        .all(&context.db)
        .await?
    {
        context
            .call(
                "delete legacy bucket policy",
                context.policies.delete_policy(policy.policy_id.as_u128()),
            )
            .await?;
        // Retain the identifier on failure; deletion in WD40 is idempotent.
        legacy_bucket_policy::Entity::delete_by_id(policy.policy_id)
            .exec(&context.db)
            .await?;
    }
    Ok(())
}

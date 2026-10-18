use policy_registry::{
    Actions, Effect, Policy, PrincipalSelector, ResourcePattern, Statement,
    traits::PolicyRegistry as _,
};
use portal_db::{
    Bucket, BucketAccess, BucketSync, CredentialAccessType, bucket, bucket_sync, credential,
    sea_orm::{
        ColumnTrait, EntityTrait, IsolationLevel, QueryFilter, QueryOrder, Set, TransactionTrait,
    },
};

use super::{Applied, Error, SyncContext};

/// Applies a bucket's policies. The caller must have persisted its sync state.
/// First application also synchronizes the organization's ALL credentials.
pub async fn sync_bucket(context: &SyncContext, bucket_name: &str) -> Result<Applied, Error> {
    let _permit = context.acquire().await?;
    let state = bucket_sync::Entity::find_by_id(bucket_name.to_owned())
        .one(&context.db)
        .await?
        .ok_or(Error::MissingState("bucket"))?;
    if state.delete_requested {
        return Err(Error::DeletionRequested);
    }
    record(context, &state, Some("sync_incomplete"), false).await?;
    let result = apply(context, &state).await;
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

async fn apply(context: &SyncContext, state: &BucketSync) -> Result<(), Error> {
    let transaction = context
        .db
        .begin_with_config(Some(IsolationLevel::Serializable), None)
        .await?;
    let current = bucket_sync::Entity::find_by_id(state.bucket_name.clone())
        .one(&transaction)
        .await?
        .ok_or(Error::StateChanged)?;
    if current.generation != state.generation
        || current.public_policy_id != state.public_policy_id
        || current.private_policy_id != state.private_policy_id
        || current.delete_requested
    {
        return Err(Error::StateChanged);
    }
    let bucket = bucket::Entity::find_by_id(state.bucket_name.clone())
        .one(&transaction)
        .await?
        .ok_or(Error::BucketNotFound)?;
    let all_credentials = if state.applied_generation == 0 {
        credential::Entity::find()
            .filter(credential::Column::OrganizationId.eq(bucket.organization_id))
            .filter(credential::Column::AccessType.eq(CredentialAccessType::All))
            .order_by_asc(credential::Column::KeyId)
            .all(&transaction)
            .await?
    } else {
        Vec::new()
    };
    transaction.commit().await?;

    let actions = match bucket.access {
        BucketAccess::Private => Actions::EMPTY,
        BucketAccess::Public => Actions::READ,
        BucketAccess::PublicList => Actions::READ | Actions::LIST,
    };
    put_policy(
        context,
        state.public_policy_id.as_u128(),
        policy(&bucket.name, actions),
    )
    .await?;
    context
        .call(
            "attach public policy",
            context.policies.attach(
                PrincipalSelector::Everyone,
                state.public_policy_id.as_u128(),
            ),
        )
        .await?;
    put_private_policy(context, &bucket, state).await?;

    for credential in all_credentials {
        // The public entry point already holds the shared semaphore.
        match super::credential::synchronize(context, &credential.key_id, None).await {
            Ok(_) | Err(Error::DeletionRequested) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn policy(bucket_name: &str, actions: Actions) -> Policy {
    Policy {
        statements: if actions.is_empty() {
            Vec::new()
        } else {
            vec![Statement {
                effect: Effect::Allow,
                actions,
                resource: ResourcePattern::Prefix(format!("{bucket_name}/")),
            }]
        },
    }
}

pub(super) async fn put_private_policy(
    context: &SyncContext,
    bucket: &Bucket,
    state: &BucketSync,
) -> Result<(), Error> {
    put_policy(
        context,
        state.private_policy_id.as_u128(),
        policy(&bucket.name, Actions::ALL),
    )
    .await
}

async fn put_policy(context: &SyncContext, id: u128, policy: Policy) -> Result<(), Error> {
    let current = context
        .call("get policy", context.policies.get_policy(id))
        .await?;
    if current.as_ref() != Some(&policy) {
        context
            .call("put policy", context.policies.put_policy(id, policy))
            .await?;
    }
    Ok(())
}

async fn record(
    context: &SyncContext,
    state: &BucketSync,
    error: Option<&str>,
    applied: bool,
) -> Result<(), Error> {
    let mut update = bucket_sync::ActiveModel {
        last_error: Set(error.map(str::to_owned)),
        ..Default::default()
    };
    if applied {
        update.applied_generation = Set(state.generation);
    }
    let result = bucket_sync::Entity::update_many()
        .set(update)
        .filter(bucket_sync::Column::BucketName.eq(state.bucket_name.clone()))
        .filter(bucket_sync::Column::Generation.eq(state.generation))
        .filter(bucket_sync::Column::PublicPolicyId.eq(state.public_policy_id))
        .filter(bucket_sync::Column::PrivatePolicyId.eq(state.private_policy_id))
        .filter(bucket_sync::Column::DeleteRequested.eq(false))
        .exec(&context.db)
        .await?;
    if result.rows_affected != 1 {
        return Err(Error::StateChanged);
    }
    Ok(())
}

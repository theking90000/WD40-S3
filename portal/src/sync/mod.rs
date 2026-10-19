//! Explicit synchronization dependencies and errors.
//!
//! Constructing or cloning the context performs no I/O and starts no tasks.
//! All operations are explicit; clones share a single application semaphore.

mod bucket;
mod credential;
mod migration;

use std::{fmt, future::Future, sync::Arc, time::Duration};

use policy_registry::{Policy, PolicyRegistry, traits::PolicyRegistry as _};
use portal_db::DatabaseConnection;
use s3_credentials_registry::S3CredentialRegistry;
use tokio::sync::{Semaphore, SemaphorePermit};

pub use bucket::sync_bucket;
pub use credential::{delete_credential, register_credential, sync_credential};
pub use migration::migrate_credential_policies;

/// The portal generation confirmed by a completed application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Applied {
    pub generation: i64,
}

#[derive(Clone)]
pub struct SyncContext {
    pub db: DatabaseConnection,
    pub credentials: S3CredentialRegistry,
    pub policies: PolicyRegistry,
    gate: Arc<Semaphore>,
}

impl SyncContext {
    pub fn new(
        db: DatabaseConnection,
        credentials: S3CredentialRegistry,
        policies: PolicyRegistry,
    ) -> Self {
        Self {
            db,
            credentials,
            policies,
            gate: Arc::new(Semaphore::new(1)),
        }
    }

    async fn acquire(&self) -> Result<SemaphorePermit<'_>, Error> {
        self.gate.acquire().await.map_err(|_| Error::Closed)
    }

    async fn call<T, E>(
        &self,
        operation: &'static str,
        future: impl Future<Output = Result<T, E>>,
    ) -> Result<T, Error>
    where
        Error: From<E>,
    {
        tokio::time::timeout(Duration::from_secs(10), future)
            .await
            .map_err(|_| Error::Timeout(operation))?
            .map_err(Error::from)
    }
}

#[derive(Debug)]
pub enum Error {
    Database(portal_db::DbErr),
    Credentials(s3_credentials_registry::Error),
    Policies(policy_registry::Error),
    MissingState(&'static str),
    BucketNotFound,
    CredentialNotFound,
    CredentialMissing,
    PrincipalMismatch,
    InvalidPermissions,
    MigrationPending,
    DeletionRequested,
    DeletionNotRequested,
    StateChanged,
    Closed,
    Timeout(&'static str),
}

impl Error {
    /// Stable codes are persisted instead of registry or database diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Database(_) => "database_error",
            Self::Credentials(_) => "credentials_registry_error",
            Self::Policies(_) => "policy_registry_error",
            Self::MissingState(_) => "sync_state_missing",
            Self::BucketNotFound => "bucket_not_found",
            Self::CredentialNotFound => "credential_not_found",
            Self::CredentialMissing => "credential_missing",
            Self::PrincipalMismatch => "principal_mismatch",
            Self::InvalidPermissions => "invalid_permissions",
            Self::MigrationPending => "policy_migration_pending",
            Self::DeletionRequested => "deletion_requested",
            Self::DeletionNotRequested => "deletion_not_requested",
            Self::StateChanged => "sync_state_changed",
            Self::Closed => "sync_closed",
            Self::Timeout(_) => "registry_timeout",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "synchronization database error: {error}"),
            Self::Credentials(error) => write!(formatter, "credentials registry error: {error}"),
            Self::Policies(error) => write!(formatter, "policy registry error: {error}"),
            Self::MissingState(resource) => {
                write!(formatter, "missing {resource} synchronization state")
            }
            Self::BucketNotFound => formatter.write_str("bucket not found"),
            Self::CredentialNotFound => formatter.write_str("credential not found"),
            Self::CredentialMissing => formatter.write_str("credential is absent from WD40"),
            Self::PrincipalMismatch => formatter.write_str("credential principal does not match"),
            Self::InvalidPermissions => formatter.write_str("invalid credential permission mask"),
            Self::MigrationPending => formatter.write_str("credential policy migration is still pending"),
            Self::DeletionRequested => formatter.write_str("resource is pending deletion"),
            Self::DeletionNotRequested => {
                formatter.write_str("credential deletion was not requested")
            }
            Self::StateChanged => {
                formatter.write_str("synchronization state changed during application")
            }
            Self::Closed => formatter.write_str("synchronization semaphore is closed"),
            Self::Timeout(operation) => {
                write!(formatter, "registry operation timed out: {operation}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Credentials(error) => Some(error),
            Self::Policies(error) => Some(error),
            _ => None,
        }
    }
}

impl From<portal_db::DbErr> for Error {
    fn from(error: portal_db::DbErr) -> Self {
        Self::Database(error)
    }
}

impl From<s3_credentials_registry::Error> for Error {
    fn from(error: s3_credentials_registry::Error) -> Self {
        Self::Credentials(error)
    }
}

impl From<policy_registry::Error> for Error {
    fn from(error: policy_registry::Error) -> Self {
        Self::Policies(error)
    }
}

async fn put_policy(context: &SyncContext, id: u128, policy: Policy) -> Result<(), Error> {
    let current = context.call("get policy", context.policies.get_policy(id)).await?;
    if current.as_ref() != Some(&policy) {
        context.call("put policy", context.policies.put_policy(id, policy)).await?;
    }
    Ok(())
}

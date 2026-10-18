//! Explicit synchronization dependencies and errors.
//!
//! Constructing or cloning the context performs no I/O and starts no tasks.
//! Synchronization operations will be called explicitly by their callers.

use std::fmt;

use policy_registry::PolicyRegistry;
use portal_db::DatabaseConnection;
use s3_credentials_registry::S3CredentialRegistry;

#[derive(Clone)]
pub struct SyncContext {
    pub db: DatabaseConnection,
    pub credentials: S3CredentialRegistry,
    pub policies: PolicyRegistry,
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
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Database(portal_db::DbErr),
    Credentials(s3_credentials_registry::Error),
    Policies(policy_registry::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "synchronization database error: {error}"),
            Self::Credentials(error) => write!(formatter, "credentials registry error: {error}"),
            Self::Policies(error) => write!(formatter, "policy registry error: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Credentials(error) => Some(error),
            Self::Policies(error) => Some(error),
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

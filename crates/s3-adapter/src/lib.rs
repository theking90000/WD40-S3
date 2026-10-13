//! S3S service skeleton for future WD40 S3 operations.

pub mod access;
pub mod auth;
#[cfg(feature = "cli")]
pub mod cli;

mod body;
mod error;
mod metadata;
mod request;
mod s3;

pub use s3s;

/// Builds an object registry key from a request's bucket and key.
#[macro_export]
macro_rules! to_key {
    ($req:expr) => {{
        let req = &$req;
        format!("{}/{}", req.bucket, req.key)
    }};
}

use access_controller::AccessController;
use blob_system::{BlobSystem, BlobWriterConfig, FrameBudget, SchedulerConfig};
use object_registry::ObjectRegistry;
use s3_credentials_registry::S3CredentialRegistry;

/// Writer and scheduler settings independent of the listener and command line.
#[derive(Clone, Debug)]
pub struct S3AdapterConfig {
    pub writer: BlobWriterConfig,
    pub scheduler: SchedulerConfig,
}

/// S3 implementation delegating operations to individual request modules.
#[derive(Clone)]
pub struct S3Adapter {
    pub config: S3AdapterConfig,
    pub credentials: S3CredentialRegistry,
    pub access: AccessController,
    pub blobs: BlobSystem,
    pub objects: ObjectRegistry,
    pub budget: FrameBudget,
}

impl S3Adapter {
    /// Constructs the adapter without starting tasks or performing I/O.
    pub fn new(
        config: S3AdapterConfig,
        credentials: S3CredentialRegistry,
        access: AccessController,
        blobs: BlobSystem,
        objects: ObjectRegistry,
        budget: FrameBudget,
    ) -> Self {
        Self {
            config,
            credentials,
            access,
            blobs,
            objects,
            budget,
        }
    }

    /// Builds a service for an application-owned Hyper or Tower server.
    /// Nonempty GET responses currently require a Tokio runtime and create one
    /// producer task. See the body module's TODO(s3-body-progress) for the planned
    /// replacement of that task/channel bridge by integrated transport polling.
    pub fn into_service(self) -> s3s::service::S3Service {
        let auth = auth::RegistryAuth::new(self.credentials.clone());
        let access = access::S3Access::new(self.credentials.clone(), self.access.clone());
        let mut builder = s3s::service::S3ServiceBuilder::new(self);
        builder.set_auth(auth);
        builder.set_access(access);
        builder.build()
    }
}

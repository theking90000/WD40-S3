//! S3S service skeleton for future WD40 S3 operations.

#[cfg(feature = "cli")]
pub mod cli;

mod request;

pub use s3s;

use access_controller::AccessController;
use blob_system::{BlobSystem, BlobWriterConfig, FrameBudget, SchedulerConfig};
use object_registry::ObjectRegistry;
use s3_credentials_registry::S3CredentialRegistry;
use s3s::{
    S3Request, S3Response, S3Result,
    dto::{HeadObjectInput, HeadObjectOutput},
};

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
    pub fn into_service(self) -> s3s::service::S3Service {
        s3s::service::S3ServiceBuilder::new(self).build()
    }
}

#[async_trait::async_trait]
impl s3s::S3 for S3Adapter {
    async fn head_object(
        &self,
        req: S3Request<HeadObjectInput>,
    ) -> S3Result<S3Response<HeadObjectOutput>> {
        request::head_object::head_object(self, req).await
    }
}

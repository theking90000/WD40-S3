//! S3S service skeleton for future WD40 S3 operations.

#[cfg(feature = "cli")]
pub mod cli;

pub use s3s;

/// Empty S3 implementation using S3S's default `NotImplemented` responses.
#[derive(Clone, Debug, Default)]
pub struct S3Adapter;

impl S3Adapter {
    /// Constructs the adapter without starting tasks or performing I/O.
    pub fn new() -> Self {
        Self
    }

    /// Builds a service for an application-owned Hyper or Tower server.
    pub fn into_service(self) -> s3s::service::S3Service {
        s3s::service::S3ServiceBuilder::new(self).build()
    }
}

impl s3s::S3 for S3Adapter {}

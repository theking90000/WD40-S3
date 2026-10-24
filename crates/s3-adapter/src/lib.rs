//! S3S service skeleton for future WD40 S3 operations.

pub mod access;
pub mod auth;
#[cfg(feature = "cli")]
pub mod cli;

mod body;
mod error;
mod metadata;
#[cfg(feature = "metrics")]
pub mod metrics;
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
#[cfg(feature = "metrics")]
use metrics_registry::MetricsRegistry;
use object_registry::ObjectRegistry;
use s3_credentials_registry::S3CredentialRegistry;
use s3_multipart_registry::S3MultipartRegistry;

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
    pub multipart: S3MultipartRegistry,
    /// Caller-owned metrics registry, shared when the adapter is cloned.
    #[cfg(feature = "metrics")]
    pub metrics: Option<MetricsRegistry>,
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
        multipart: S3MultipartRegistry,
        budget: FrameBudget,
    ) -> Self {
        Self {
            config,
            credentials,
            access,
            blobs,
            objects,
            multipart,
            #[cfg(feature = "metrics")]
            metrics: None,
            budget,
        }
    }

    /// Attaches an opened metrics registry without performing I/O.
    #[cfg(feature = "metrics")]
    pub fn with_metrics(mut self, metrics: MetricsRegistry) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Starts one metrics task when a registry is attached.
    ///
    /// The context is built only when metrics are enabled. The current Tokio
    /// runtime must have its time driver enabled. Keep the returned tracker
    /// until the operation, including any response body, has finished.
    #[cfg(feature = "metrics")]
    pub fn request_metrics(
        &self,
        context: impl FnOnce() -> metrics::MetricsContext,
    ) -> std::io::Result<Option<metrics::RequestMetrics>> {
        self.metrics
            .as_ref()
            .map(|registry| metrics::RequestMetrics::start(registry, context()))
            .transpose()
    }

    /// Publishes prepared object metadata through the request task when tracked.
    pub(crate) async fn store_object(
        &self,
        key: &str,
        blob_id: blob_system::BlobId,
        metadata: object_registry::Metadata,
        #[cfg(feature = "metrics")] metrics: &mut Option<metrics::RequestMetrics>,
    ) -> s3s::S3Result<object_registry::StoreResult> {
        #[cfg(feature = "metrics")]
        if let Some(metrics) = metrics.as_mut() {
            return metrics
                .store_object(&self.objects, key, blob_id, metadata)
                .await;
        }
        object_registry::traits::ObjectRegistry::store(&self.objects, key, blob_id, metadata)
            .await
            .map_err(error::object_registry_error)
    }

    pub(crate) async fn delete_object_version(
        &self,
        key: &str,
        version_id: Option<object_registry::VersionId>,
        #[cfg(feature = "metrics")] metrics: &mut Option<metrics::RequestMetrics>,
    ) -> s3s::S3Result<object_registry::DeleteResult> {
        #[cfg(feature = "metrics")]
        if let Some(metrics) = metrics.as_mut() {
            return metrics
                .delete_object_version(&self.objects, key, version_id)
                .await;
        }
        object_registry::traits::ObjectRegistry::delete(&self.objects, key, version_id)
            .await
            .map_err(error::object_registry_error)
    }

    /// Builds a service for an application-owned Hyper or Tower server.
    /// Nonempty GET responses currently require a Tokio runtime and create one
    /// producer task. See the body module's TODO(s3-body-progress) for the planned
    /// replacement of that task/channel bridge by integrated transport polling.
    pub fn into_service(self) -> s3s::service::S3Service {
        self.service_builder().build()
    }

    /// Builds an authenticated service with caller-supplied virtual-host parsing.
    pub fn into_service_with_host(self, host: impl s3s::host::S3Host) -> s3s::service::S3Service {
        let mut builder = self.service_builder();
        builder.set_host(host);
        builder.build()
    }

    fn service_builder(self) -> s3s::service::S3ServiceBuilder {
        let auth = auth::RegistryAuth::new(self.credentials.clone());
        let access = access::S3Access::new(self.credentials.clone(), self.access.clone());
        let mut builder = s3s::service::S3ServiceBuilder::new(self);
        builder.set_auth(auth);
        builder.set_access(access);
        builder
    }
}

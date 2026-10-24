//! Final object mutations, including accounting handoff after a successful commit.

use object_registry::{
    BlobId, DeleteResult, Metadata, StoreResult, VersionId, traits::ObjectRegistry as _,
};
use s3s::S3Result;

#[cfg(feature = "metrics")]
use crate::metrics::{RequestMetrics, stored};
use crate::{S3Adapter, error};

impl S3Adapter {
    /// With accounting enabled, the commit and delta handoff survive cancellation.
    pub(crate) async fn store_object(
        &self,
        key: &str,
        blob_id: BlobId,
        metadata: Metadata,
        #[cfg(feature = "metrics")] metrics: &mut Option<RequestMetrics>,
    ) -> S3Result<StoreResult> {
        #[cfg(feature = "metrics")]
        if let Some(metrics) = metrics.as_ref() {
            let next = stored::Contribution::from_metadata(&metadata)?;
            let objects = self.objects.clone();
            let key = key.to_owned();
            let updates = metrics.stored_updates();
            return complete_independently(async move {
                let result = objects
                    .store(&key, blob_id, metadata)
                    .await
                    .map_err(error::object_registry_error)?;
                submit(
                    updates,
                    stored::store_records(&key, result.previous.as_ref(), &next),
                )
                .await;
                Ok(result)
            })
            .await;
        }
        self.objects
            .store(key, blob_id, metadata)
            .await
            .map_err(error::object_registry_error)
    }

    pub(crate) async fn delete_object_version(
        &self,
        key: &str,
        version_id: Option<VersionId>,
        #[cfg(feature = "metrics")] metrics: &mut Option<RequestMetrics>,
    ) -> S3Result<DeleteResult> {
        #[cfg(feature = "metrics")]
        if let Some(metrics) = metrics.as_ref() {
            let objects = self.objects.clone();
            let key = key.to_owned();
            let updates = metrics.stored_updates();
            return complete_independently(async move {
                let result = objects
                    .delete(&key, version_id)
                    .await
                    .map_err(error::object_registry_error)?;
                submit(updates, stored::delete_records(&key, &result)).await;
                Ok(result)
            })
            .await;
        }
        self.objects
            .delete(key, version_id)
            .await
            .map_err(error::object_registry_error)
    }
}

/// Dropping the join handle detaches this phase instead of cancelling its future.
#[cfg(feature = "metrics")]
async fn complete_independently<T: Send + 'static>(
    mutation: impl std::future::Future<Output = S3Result<T>> + Send + 'static,
) -> S3Result<T> {
    let runtime = tokio::runtime::Handle::try_current().map_err(s3s::S3Error::internal_error)?;
    runtime
        .spawn(mutation)
        .await
        .map_err(s3s::S3Error::internal_error)?
}

#[cfg(feature = "metrics")]
async fn submit(
    updates: tokio::sync::mpsc::Sender<Vec<metrics_registry::Record>>,
    records: S3Result<Vec<metrics_registry::Record>>,
) {
    // A registry error can follow a commit without returning snapshots. That
    // case cannot reach this handoff or provide enough information for a delta.
    let records = match records {
        Ok(records) => records,
        Err(error) => {
            eprintln!("S3 stored-object accounting failed: {error}");
            return;
        }
    };
    if !records.is_empty() && updates.send(records).await.is_err() {
        eprintln!("S3 stored-object accounting failed: metrics publication task stopped");
    }
}

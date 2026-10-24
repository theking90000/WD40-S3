//! Metrics tracking with one independent publication task per request.
//!
//! A tracker accepts both upload and download bytes without knowing the S3
//! operation. It has one owner and must follow the response body for streaming
//! requests. Only its task shares the atomic totals used for periodic snapshots.
//! Explicit completion determines success or failure; dropping an unfinished
//! tracker sends its final totals and reports failure without asynchronous work.
//!
//! S3 routing starts each tracker and gives ownership to its handler. The handler
//! decides when to finish or transfers ownership into a streaming response.
//! StoredBytes mutations run in this same task independently of request outcomes.
//! Queueing transfers ownership, so abandoning a handler cannot cancel a queued
//! store or delete between its commit and metric publication.
//!
//! StoredBytes records changes, not an automatic inventory. Before enabling it
//! over existing data, establish a one-time baseline without concurrent writers.
//! Re-enabling collection after unrecorded mutations also requires reconciliation.
//! Legacy versions without attribution contribute only to the bucket namespace.

use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};

use metrics_registry::MetricsRegistry;
use object_registry::{BlobId, DeleteResult, Metadata, ObjectRegistry, StoreResult, VersionId};
use s3s::{S3Error, S3Result};
use tokio::sync::{mpsc, oneshot};

mod context;
mod stored;
mod task;

pub use context::MetricsContext;

/// Outcome of the whole operation, including any streaming response body.
#[derive(Clone, Copy, Debug)]
pub enum RequestOutcome {
    Success,
    Failed,
}

#[derive(Clone, Copy, Debug, Default)]
struct Totals {
    uploaded: i64,
    downloaded: i64,
}

#[derive(Default)]
struct SharedTotals {
    uploaded: AtomicI64,
    downloaded: AtomicI64,
}

impl SharedTotals {
    fn snapshot(&self) -> Totals {
        // These values publish no other memory. Exact final totals travel in
        // the oneshot message, independently of these periodic observations.
        Totals {
            uploaded: self.uploaded.load(Ordering::Relaxed),
            downloaded: self.downloaded.load(Ordering::Relaxed),
        }
    }
}

struct Completion {
    totals: Totals,
    outcome: RequestOutcome,
}

/// Owned context, byte counters and completion signal for one request.
///
/// This type deliberately does not implement Clone. Moving it into a response
/// body transfers responsibility for completion. A forgotten explicit completion
/// is counted as failure when the tracker is dropped.
pub struct RequestMetrics {
    context: Arc<MetricsContext>,
    totals: Totals,
    shared: Arc<SharedTotals>,
    completion: Option<oneshot::Sender<Completion>>,
    mutations: mpsc::Sender<stored::Mutation>,
}

impl RequestMetrics {
    /// Starts one task, publishing byte increments every five seconds and on
    /// completion. Requires a Tokio runtime with its time driver enabled.
    /// Failure to find a current runtime is returned as an I/O error.
    pub fn start(registry: &MetricsRegistry, context: MetricsContext) -> io::Result<Self> {
        let runtime = tokio::runtime::Handle::try_current().map_err(io::Error::other)?;
        let context = Arc::new(context);
        let shared = Arc::new(SharedTotals::default());
        let (sender, receiver) = oneshot::channel();
        let (mutations, queued) = mpsc::channel(1);

        runtime.spawn(task::run(
            registry.clone(),
            Arc::clone(&context),
            Arc::clone(&shared),
            receiver,
            queued,
        ));

        Ok(Self {
            context,
            totals: Totals::default(),
            shared,
            completion: Some(sender),
            mutations,
        })
    }

    /// Returns the immutable attribution carried by this request.
    pub fn context(&self) -> &MetricsContext {
        &self.context
    }

    pub(crate) async fn store_object(
        &mut self,
        objects: &ObjectRegistry,
        key: &str,
        blob_id: BlobId,
        metadata: Metadata,
    ) -> S3Result<StoreResult> {
        let (reply, result) = oneshot::channel();
        self.mutations
            .send(stored::Mutation::Store {
                objects: objects.clone(),
                key: key.to_owned(),
                blob_id,
                metadata,
                reply,
            })
            .await
            .map_err(|_| s3s::s3_error!(InternalError, "metrics mutation task stopped"))?;
        result.await.map_err(S3Error::internal_error)?
    }

    pub(crate) async fn delete_object_version(
        &mut self,
        objects: &ObjectRegistry,
        key: &str,
        version_id: Option<VersionId>,
    ) -> S3Result<DeleteResult> {
        let (reply, result) = oneshot::channel();
        self.mutations
            .send(stored::Mutation::Delete {
                objects: objects.clone(),
                key: key.to_owned(),
                version_id,
                reply,
            })
            .await
            .map_err(|_| s3s::s3_error!(InternalError, "metrics mutation task stopped"))?;
        result.await.map_err(S3Error::internal_error)?
    }

    /// Counts bytes consumed from the incoming body, before backend writes.
    pub fn add_uploaded_bytes(&mut self, count: usize) -> io::Result<()> {
        let count = i64::try_from(count).map_err(io::Error::other)?;
        self.totals.uploaded = self.totals.uploaded.checked_add(count).ok_or_else(|| {
            io::Error::other("uploaded byte total exceeds the metrics integer range")
        })?;
        self.shared
            .uploaded
            .store(self.totals.uploaded, Ordering::Relaxed);
        Ok(())
    }

    /// Counts bytes handed to the outgoing body consumer, excluding prefetch.
    pub fn add_downloaded_bytes(&mut self, count: usize) -> io::Result<()> {
        let count = i64::try_from(count).map_err(io::Error::other)?;
        self.totals.downloaded = self.totals.downloaded.checked_add(count).ok_or_else(|| {
            io::Error::other("downloaded byte total exceeds the metrics integer range")
        })?;
        self.shared
            .downloaded
            .store(self.totals.downloaded, Ordering::Relaxed);
        Ok(())
    }

    /// Signals the final outcome and transfers remaining totals synchronously.
    /// Publication continues in the independent task after this method returns.
    pub fn finish(mut self, outcome: RequestOutcome) {
        self.complete(outcome);
    }

    fn complete(&mut self, outcome: RequestOutcome) {
        if let Some(sender) = self.completion.take() {
            let _ = sender.send(Completion {
                totals: self.totals,
                outcome,
            });
        }
    }
}

impl Drop for RequestMetrics {
    fn drop(&mut self) {
        self.complete(RequestOutcome::Failed);
    }
}

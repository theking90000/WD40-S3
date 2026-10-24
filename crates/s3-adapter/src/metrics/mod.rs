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
//! StoredBytes updates remain separate from transfer counters and outcomes.

use std::{
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    task::{Context, Poll},
};

use bytes::Bytes;
use futures_util::Stream;
use metrics_registry::MetricsRegistry;
use s3s::{
    StdError,
    dto::StreamingBlob,
    stream::{ByteStream, RemainingLength},
};
use tokio::sync::oneshot;

mod context;
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

        runtime.spawn(task::run(
            registry.clone(),
            Arc::clone(&context),
            Arc::clone(&shared),
            receiver,
        ));

        Ok(Self {
            context,
            totals: Totals::default(),
            shared,
            completion: Some(sender),
        })
    }

    /// Returns the immutable attribution carried by this request.
    pub fn context(&self) -> &MetricsContext {
        &self.context
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

    /// Moves completion into the outgoing body, after backend prefetch.
    pub(crate) fn track_download(self, body: StreamingBlob) -> StreamingBlob {
        // HTTP may discard a known-empty body without ever polling it.
        if body.remaining_length().exact() == Some(0) {
            self.finish(RequestOutcome::Success);
            return body;
        }
        StreamingBlob::new(DownloadBody {
            body,
            metrics: Some(self),
        })
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

/// Counts only chunks returned to the consumer. Dropping this body also drops
/// its unfinished tracker, sending the final totals with a failed outcome.
struct DownloadBody {
    body: StreamingBlob,
    metrics: Option<RequestMetrics>,
}

impl DownloadBody {
    fn finish(&mut self, outcome: RequestOutcome) {
        if let Some(metrics) = self.metrics.take() {
            metrics.finish(outcome);
        }
    }
}

impl Stream for DownloadBody {
    type Item = Result<Bytes, StdError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.metrics.is_none() {
            return Poll::Ready(None);
        }
        match Pin::new(&mut this.body).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                if let Some(metrics) = this.metrics.as_mut() {
                    if let Err(error) = metrics.add_downloaded_bytes(bytes.len()) {
                        this.finish(RequestOutcome::Failed);
                        return Poll::Ready(Some(Err(Box::new(error))));
                    }
                }
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.finish(RequestOutcome::Failed);
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                this.finish(RequestOutcome::Success);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.metrics.is_none() {
            (0, Some(0))
        } else {
            self.body.size_hint()
        }
    }
}

impl ByteStream for DownloadBody {
    fn remaining_length(&self) -> RemainingLength {
        if self.metrics.is_none() {
            RemainingLength::new_exact(0)
        } else {
            self.body.remaining_length()
        }
    }
}

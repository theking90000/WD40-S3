//! Independent persistence of a single request's counters and final outcome.

use std::{io, sync::Arc, time::Duration};

use metrics_registry::{
    Metric, MetricKind, MetricRecord, MetricsRegistry, traits::MetricsRegistry as _,
};
use tokio::sync::{mpsc, oneshot};

use super::stored::Mutation;
use super::{Completion, MetricsContext, RequestOutcome, SharedTotals, Totals};

const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

enum Event {
    Complete(Completion),
    Mutation(Option<Mutation>),
    Flush,
}

pub(super) async fn run(
    registry: MetricsRegistry,
    context: Arc<MetricsContext>,
    shared: Arc<SharedTotals>,
    mut completion: oneshot::Receiver<Completion>,
    mut mutations: mpsc::Receiver<Mutation>,
) {
    publish(&registry, &context, vec![counter(Metric::RequestCount, 1)]).await;

    let mut previous = Totals::default();
    let mut mutations_open = true;
    loop {
        // Select only the next observation. Once a registry write starts, it is
        // awaited to completion even if the request ends during that write.
        let event = tokio::select! {
            biased;
            result = &mut completion => Event::Complete(result.unwrap_or_else(|_| Completion {
                totals: shared.snapshot(),
                outcome: RequestOutcome::Failed,
            })),
            mutation = mutations.recv(), if mutations_open => Event::Mutation(mutation),
            _ = tokio::time::sleep(FLUSH_INTERVAL) => Event::Flush,
        };
        let finished = match event {
            Event::Mutation(Some(mutation)) => {
                mutation.apply(&registry).await;
                continue;
            }
            Event::Mutation(None) => {
                mutations_open = false;
                continue;
            }
            Event::Complete(end) => {
                // Cancellation must not discard a mutation already accepted by
                // the bounded queue. Finish those before publishing the outcome.
                mutations.close();
                while let Some(mutation) = mutations.recv().await {
                    mutation.apply(&registry).await;
                }
                Some(end)
            }
            Event::Flush => None,
        };
        let totals = finished
            .as_ref()
            .map_or_else(|| shared.snapshot(), |end| end.totals);
        let mut metrics = match byte_increments(totals, previous) {
            Ok(metrics) => metrics,
            Err(error) => {
                eprintln!("S3 metrics accounting failed: {error}");
                return;
            }
        };

        if let Some(end) = &finished {
            let metric = match end.outcome {
                RequestOutcome::Success => Metric::SuccessRequestCount,
                RequestOutcome::Failed => Metric::FailedRequestCount,
            };
            metrics.push(counter(metric, 1));
        }

        publish(&registry, &context, metrics).await;
        // Backend errors do not imply rollback. Advance after each attempt so
        // an ambiguous counter increment is never automatically replayed.
        previous = totals;
        if finished.is_some() {
            return;
        }
    }
}

fn byte_increments(totals: Totals, previous: Totals) -> io::Result<Vec<MetricRecord>> {
    let mut metrics = Vec::with_capacity(3);
    for (metric, total, before) in [
        (Metric::UploadedBytes, totals.uploaded, previous.uploaded),
        (
            Metric::DownloadedBytes,
            totals.downloaded,
            previous.downloaded,
        ),
    ] {
        let delta = total
            .checked_sub(before)
            .filter(|delta| *delta >= 0)
            .ok_or_else(|| io::Error::other("request byte totals must not decrease"))?;
        if delta != 0 {
            metrics.push(counter(metric, delta));
        }
    }
    Ok(metrics)
}

fn counter(metric: Metric, value: i64) -> MetricRecord {
    MetricRecord {
        metric,
        kind: MetricKind::Counter,
        value,
    }
}

async fn publish(registry: &MetricsRegistry, context: &MetricsContext, metrics: Vec<MetricRecord>) {
    if metrics.is_empty() {
        return;
    }
    if let Err(error) = registry.record(context.records(metrics)).await {
        eprintln!("S3 metrics recording failed: {error}");
    }
}

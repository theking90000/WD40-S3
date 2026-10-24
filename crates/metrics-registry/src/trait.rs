//! Batch inputs, query results and the metrics registry contract.

use std::{
    future::Future,
    ops::Range,
    time::{Duration, SystemTime},
};

use crate::Error;

/// How a recorded value changes a metric.
///
/// Gauges receive signed deltas; counters receive nonnegative increments.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum MetricKind {
    /// Add a signed delta to a current value initially equal to zero.
    Gauge,
    /// Add a nonnegative amount to the counter's historical windows.
    Counter,
}

/// One gauge delta or counter increment for a metric and kind.
///
/// Metric identifiers are converted into `u16` when recording the owned batch.
/// Custom metric types only need to implement `Into<u16>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricRecord<Metric = crate::Metric> {
    /// Metric to update.
    pub metric: Metric,
    /// Whether `value` is a signed gauge delta or a nonnegative counter increment.
    pub kind: MetricKind,
    /// Signed delta for a gauge, or a nonnegative increment for a counter.
    ///
    /// Zero is a no-op for both kinds. A gauge may become negative.
    pub value: i64,
}

/// Mutations sharing a key and the aggregate prefixes to maintain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record<Metric = crate::Metric> {
    /// Opaque, nonempty key, without application-specific semantics.
    pub key: String,
    /// Ancestor string prefixes, applied independently to every metric below.
    ///
    /// Each prefix must match the beginning of `key`.
    /// Empty prefixes select the whole namespace; duplicates count only once.
    /// A requested prefix is maintained across all matching updates of the same
    /// metric and kind in the batch, including updates from other records.
    pub aggregates: Vec<String>,
    /// Metric updates to apply to this key.
    pub metrics: Vec<MetricRecord<Metric>>,
}

impl<Metric: Into<u16>> Record<Metric> {
    pub(crate) fn into_ids(self) -> Record<u16> {
        Record {
            key: self.key,
            aggregates: self.aggregates,
            metrics: self
                .metrics
                .into_iter()
                .map(|metric| MetricRecord {
                    metric: metric.metric.into(),
                    kind: metric.kind,
                    value: metric.value,
                })
                .collect(),
        }
    }
}

/// Which current or historical values a query requests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    /// The exact gauge current.
    ///
    /// Counters have no resolution-independent current in this API.
    Current,
    /// Historical values at a configured resolution over an absolute time range.
    History {
        /// A nonzero, configured window duration.
        resolution: Duration,
        /// Absolute interval, start inclusive and end exclusive.
        ///
        /// Intersecting windows are returned whole and aligned to the Unix epoch.
        range: Range<SystemTime>,
    },
}

/// One metric and the current or historical values to read for it.
///
/// Metric identifiers are converted into `u16` when querying the owned batch.
/// Custom metric types only need to implement `Into<u16>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricQuery<Metric = crate::Metric> {
    /// Metric to read.
    pub metric: Metric,
    /// Whether the metric is a gauge or a counter.
    pub kind: MetricKind,
    /// Current values or a historical time range.
    pub selection: Selection,
}

/// Metrics to read over the union of the supplied string prefixes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query<Metric = crate::Metric> {
    /// Overlapping and duplicate prefixes never double-count a key.
    ///
    /// An empty list selects no keys; an empty string selects every key.
    pub prefixes: Vec<String>,
    /// Metric selections, in the order expected in [`QueryResult::metrics`].
    pub metrics: Vec<MetricQuery<Metric>>,
}

impl<Metric: Into<u16>> Query<Metric> {
    pub(crate) fn into_ids(self) -> Query<u16> {
        Query {
            prefixes: self.prefixes,
            metrics: self
                .metrics
                .into_iter()
                .map(|metric| MetricQuery {
                    metric: metric.metric.into(),
                    kind: metric.kind,
                    selection: metric.selection,
                })
                .collect(),
        }
    }
}

/// A metric's value for one historical window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point {
    /// Beginning of the window, aligned to the Unix epoch at the query resolution.
    pub start: SystemTime,
    /// End-of-window gauge value or the counter total within this window.
    ///
    /// For the open window, gauges report their current value.
    pub value: i64,
    /// False only for the window currently in progress.
    pub complete: bool,
}

/// Values returned for one [`MetricQuery`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetricResult {
    /// The exact gauge current requested by [`Selection::Current`].
    Current(i64),
    /// Historical points ordered by increasing start time.
    History(Vec<Point>),
}

/// Results for one [`Query`], preserving its metric order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryResult {
    /// Same length and order as the corresponding query's metrics.
    pub metrics: Vec<MetricResult>,
}

/// Persistent batch recording and querying over a namespace of keys.
///
/// Implementations manage time, aggregate caches and historical retention.
/// Callers submit gauge deltas or counter increments and request current or
/// historical values. See the [crate example](crate) for a complete batch.
/// Batches are consumed so metric identifiers only require `Into<u16>`, without
/// `Copy`, `Clone`, `Send` or `Sync`. Conversion happens before returning the future.
/// Identifiers must be stable and distinct within the namespace.
pub trait MetricsRegistry {
    /// Records signed gauge deltas and counter increments as one atomic batch.
    ///
    /// The whole batch is validated before applying mutations. Success awaits
    /// durable persistence, and timestamps are assigned internally using system time.
    /// Empty batches succeed without changes. Zero deltas create no buckets.
    /// Repeated mutations of the same key, metric and kind are added together;
    /// a net-zero group has no effect.
    ///
    /// Gauges start at zero and accumulate every signed delta. The same delta
    /// updates the current value and each configured historical window atomically.
    /// Historical queries accumulate these windows to return end-of-window values,
    /// rather than exposing the deltas. Timestamps only select the receiving
    /// backend's windows; there is no last-write-wins rule or caller timestamp.
    /// Deltas are not idempotent. Replaying a batch applies its increments again.
    ///
    /// Requested aggregate caches are maintained by addition when valid and
    /// rebuilt internally when needed. Other affected caches are invalidated.
    /// Cache reconstruction must remain correct under concurrent writes.
    /// Gauge totals, counter totals and aggregates use a wider internal integer.
    /// Native additions can detect stored-total overflow only when evaluated;
    /// output conversion to `i64` is checked by [`Self::query`].
    /// Historical retention is implementation-owned and never expires a gauge's
    /// current value. See the backend documentation for cleanup availability.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRecord`] when an input violates the record contract,
    /// including a negative counter increment or an aggregate that is not a key
    /// prefix. Returns [`Error::Overflow`] when input grouping
    /// cannot be represented, or [`Error::Backend`] when an operation fails.
    /// Backend failures do not guarantee that the batch was rolled back.
    fn record<Metric: Into<u16>>(
        &self,
        records: Vec<Record<Metric>>,
    ) -> impl Future<Output = Result<(), Error>> + Send;

    /// Reads a batch of metric selections from one consistent view.
    ///
    /// Results preserve query order and the metric order within each query.
    /// One invalid query rejects the batch. Empty batches return no results.
    /// Valid caches and source reads must produce the same values.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidQuery`] when a selection or time range is invalid,
    /// [`Error::ResolutionUnavailable`] when the requested resolution is not
    /// available, or [`Error::HistoryExpired`] when the requested precision is no
    /// longer retained. Returns [`Error::Overflow`] when a result cannot be
    /// represented or an evaluated native total has overflowed, or
    /// [`Error::Backend`] when an implementation operation fails.
    fn query<Metric: Into<u16>>(
        &self,
        queries: Vec<Query<Metric>>,
    ) -> impl Future<Output = Result<Vec<QueryResult>, Error>> + Send;
}

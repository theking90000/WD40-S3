//! Batch inputs, query results and the metrics registry contract.

use std::{
    future::Future,
    ops::Range,
    time::{Duration, SystemTime},
};

use crate::{Error, Metric};

/// How a recorded value changes a metric.
///
/// Gauges receive absolute values; counters receive nonnegative increments.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum MetricKind {
    /// Replace the current value using an internally timestamped assignment.
    Gauge,
    /// Add a nonnegative amount to the counter's historical windows.
    Counter,
}

/// One gauge assignment or counter increment for a metric and kind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricRecord {
    /// Metric to update.
    pub metric: Metric,
    /// Whether `value` replaces a gauge or increments a counter.
    pub kind: MetricKind,
    /// Absolute value for a gauge, or a nonnegative increment for a counter.
    ///
    /// Assigning zero to a gauge may decrease its previous value.
    pub value: i64,
}

/// Mutations sharing a key and the aggregate prefixes to maintain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
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
    pub metrics: Vec<MetricRecord>,
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricQuery {
    /// Metric to read.
    pub metric: Metric,
    /// Whether the metric is a gauge or a counter.
    pub kind: MetricKind,
    /// Current values or a historical time range.
    pub selection: Selection,
}

/// Metrics to read over the union of the supplied string prefixes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query {
    /// Overlapping and duplicate prefixes never double-count a key.
    ///
    /// An empty list selects no keys; an empty string selects every key.
    pub prefixes: Vec<String>,
    /// Metric selections, in the order expected in [`QueryResult::metrics`].
    pub metrics: Vec<MetricQuery>,
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
/// Callers submit gauge assignments or counter increments and request current or
/// historical values. See the [crate example](crate) for a complete batch.
pub trait MetricsRegistry {
    /// Records gauge assignments and counter increments as one atomic batch.
    ///
    /// The whole batch is validated before applying mutations. Success awaits
    /// durable persistence, and timestamps are assigned internally using system time.
    /// Empty batches succeed without changes. Zero counter increments create no
    /// buckets; assigning zero to a gauge is still an assignment.
    ///
    /// Gauge values carry an internal timestamp, with an internal tie-breaker for
    /// equal timestamps. The newest assignment wins; repeated assignments to a
    /// gauge within one batch follow input order. No timestamp is caller-supplied.
    /// Each winning assignment atomically replaces the current value and the
    /// absolute gauge value in its historical windows. Aggregate maintenance uses
    /// the difference from the previous accepted value. Superseded assignments
    /// must not alter history or aggregates.
    ///
    /// Requested aggregate caches are maintained by addition when valid and
    /// rebuilt internally when needed. Other affected caches are invalidated.
    /// Cache reconstruction must remain correct under concurrent writes.
    /// Counter totals and aggregate calculations use a wider internal integer.
    /// Native additions can detect stored-total overflow only when evaluated;
    /// output conversion to `i64` is checked by [`Self::query`].
    /// Historical retention is implementation-owned and never expires a gauge's
    /// current value. See the backend documentation for cleanup availability.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRecord`] when an input violates the record contract,
    /// including a negative counter increment or an aggregate that is not a key
    /// prefix. Returns [`Error::Overflow`] when input grouping or gauge preparation
    /// cannot be represented, or [`Error::Backend`] when an operation fails.
    /// Backend failures do not guarantee that the batch was rolled back.
    fn record(&self, records: &[Record]) -> impl Future<Output = Result<(), Error>> + Send;

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
    fn query(
        &self,
        queries: &[Query],
    ) -> impl Future<Output = Result<Vec<QueryResult>, Error>> + Send;
}

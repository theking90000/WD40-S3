use std::{
    future::Future,
    ops::Range,
    time::{Duration, SystemTime},
};

use crate::{Error, Metric};

/// Gauges receive absolute values; counters receive nonnegative increments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetricKind {
    /// Replace the current value using an internally timestamped assignment.
    Gauge,
    /// Add a nonnegative amount to the counter's historical windows.
    Counter,
}

/// One gauge assignment or counter increment for a metric and kind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricRecord {
    pub metric: Metric,
    pub kind: MetricKind,
    /// Absolute value for a gauge, or a nonnegative increment for a counter.
    /// Assigning zero to a gauge may decrease its previous value.
    pub value: i64,
}

/// Mutations sharing a key and the aggregate prefixes to maintain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    /// Opaque, nonempty key, without application-specific semantics.
    pub key: String,
    /// Ancestor string prefixes, applied independently to every metric below.
    /// Empty prefixes select the whole namespace; duplicates count only once.
    pub aggregates: Vec<String>,
    pub metrics: Vec<MetricRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    /// Exact gauge current. Counters have no resolution-independent current.
    Current,
    History {
        /// A nonzero, configured window duration.
        resolution: Duration,
        /// Absolute interval, start inclusive and end exclusive.
        /// Intersecting windows are returned whole and aligned to the Unix epoch.
        range: Range<SystemTime>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricQuery {
    pub metric: Metric,
    pub kind: MetricKind,
    pub selection: Selection,
}

/// Metrics to read over the union of the supplied string prefixes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Query {
    /// Overlapping and duplicate prefixes never double-count a key.
    /// An empty list selects no keys; an empty string selects every key.
    pub prefixes: Vec<String>,
    pub metrics: Vec<MetricQuery>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point {
    /// Beginning of the window, aligned to the Unix epoch at the query resolution.
    pub start: SystemTime,
    /// End-of-window gauge value or the counter total within this window.
    /// For the open window, gauges report their current value.
    pub value: i64,
    /// False only for the window currently in progress.
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetricResult {
    Current(i64),
    /// One point per intersecting window, ordered by increasing start time.
    History(Vec<Point>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryResult {
    /// Same length and order as the corresponding query's metrics.
    pub metrics: Vec<MetricResult>,
}

pub trait MetricsRegistry {
    /// Validates and atomically applies the entire batch using system time.
    /// Success awaits durable persistence; backend errors need not mean rollback.
    /// Empty batches succeed without changes. Zero counter increments create no
    /// buckets; assigning zero to a gauge is still an assignment.
    ///
    /// Gauge values carry an internal timestamp, with an internal tie-breaker for
    /// equal timestamps. The newest assignment wins; repeated assignments to a
    /// gauge within one batch follow input order. No timestamp is caller-supplied.
    /// For each winning assignment, the implementation atomically derives the
    /// change from the previous accepted value and applies that change to history
    /// and aggregates. Superseded assignments must not alter those sums.
    ///
    /// Requested aggregate caches are maintained by addition when valid and
    /// rebuilt internally when needed. Other affected caches are invalidated.
    /// Cache reconstruction must remain correct under concurrent writes.
    /// Historical retention and physical cleanup are implementation-owned and
    /// never expire a gauge's current value.
    fn record(&self, records: &[Record]) -> impl Future<Output = Result<(), Error>> + Send;

    /// Reads the whole batch from one consistent view. Results preserve request
    /// order; one invalid query rejects the batch. Empty batches return no results.
    /// Valid caches and source reads must produce the same values.
    fn query(
        &self,
        queries: &[Query],
    ) -> impl Future<Output = Result<Vec<QueryResult>, Error>> + Send;
}

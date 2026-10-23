//! Batched gauge assignments and counter increments over one namespace of keys.
//!
//! Implementations own persistence, system time, aggregate caches and retention.
//! The public API consists only of recording mutations and querying values.

pub mod error;
pub mod metrics;
#[path = "trait.rs"]
pub mod traits;

pub use error::Error;
pub use metrics::Metric;
pub use traits::{
    MetricKind, MetricQuery, MetricRecord, MetricResult, MetricsRegistry, Point, Query,
    QueryResult, Record, Selection,
};

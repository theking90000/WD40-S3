//! Contracts for recording and querying metrics in batches over a namespace of keys.
//!
//! [`MetricsRegistry`] accepts absolute gauge values and counter increments through
//! [`Record`] batches, and reads current or historical values through [`Query`]
//! batches. Implementations own persistence, system time, caches and retention.
//! This crate defines the API without providing a storage implementation.
//!
//! # Examples
//!
//! Record stored bytes and a request count for one key, then query the stored-byte
//! current and request history using a caller-supplied registry implementation.
//!
//! ```
//! use std::time::{Duration, SystemTime};
//!
//! use metrics_registry::{
//!     Error, Metric, MetricKind, MetricQuery, MetricRecord, MetricsRegistry,
//!     Query, QueryResult, Record, Selection,
//! };
//!
//! async fn record_and_query(
//!     registry: &impl MetricsRegistry,
//! ) -> Result<Vec<QueryResult>, Error> {
//!     let records = [Record {
//!         key: "/buckets/photos/object-a".to_owned(),
//!         aggregates: vec!["/buckets/photos/".to_owned()],
//!         metrics: vec![
//!             MetricRecord {
//!                 metric: Metric::StoredBytes,
//!                 kind: MetricKind::Gauge,
//!                 value: 150,
//!             },
//!             MetricRecord {
//!                 metric: Metric::RequestCount,
//!                 kind: MetricKind::Counter,
//!                 value: 1,
//!             },
//!         ],
//!     }];
//!     registry.record(&records).await?;
//!
//!     let end = SystemTime::now();
//!     let start = end
//!         .checked_sub(Duration::from_secs(3600))
//!         .ok_or(Error::Overflow)?;
//!     let queries = [Query {
//!         prefixes: vec!["/buckets/photos/".to_owned()],
//!         metrics: vec![
//!             MetricQuery {
//!                 metric: Metric::StoredBytes,
//!                 kind: MetricKind::Gauge,
//!                 selection: Selection::Current,
//!             },
//!             MetricQuery {
//!                 metric: Metric::RequestCount,
//!                 kind: MetricKind::Counter,
//!                 selection: Selection::History {
//!                     resolution: Duration::from_secs(300),
//!                     range: start..end,
//!                 },
//!             },
//!         ],
//!     }];
//!     registry.query(&queries).await
//! }
//! ```

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

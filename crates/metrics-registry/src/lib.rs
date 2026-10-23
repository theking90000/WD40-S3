//! Batch recording and querying interfaces over a namespace of keys.
//!
//! [`MetricsRegistry`] accepts absolute gauge values and counter increments through
//! [`Record`] batches, and reads current or historical values through [`Query`]
//! batches. Implementations own persistence, system time, caches and retention.
//! [`LocalRegistry::new`] accepts an already opened RocksDB database and a shared
//! retention [`Config`]. Recording and querying are not implemented yet and return
//! [`Error::LocalUnavailable`].
//!
//! # Examples
//!
//! Record stored bytes and a request count for one key, then query the stored-byte
//! current and request history through the registry enum. These operations return
//! [`Error::LocalUnavailable`] until the local backend is implemented.
//!
//! ```
//! use std::time::{Duration, SystemTime};
//!
//! use metrics_registry::{
//!     Error, Metric, MetricKind, MetricQuery, MetricRecord, MetricsRegistry,
//!     Query, QueryResult, Record, Selection,
//! };
//! use metrics_registry::traits::MetricsRegistry as _;
//!
//! async fn record_and_query(
//!     registry: &MetricsRegistry,
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

pub mod config;
pub mod error;
pub mod local;
pub mod metrics;
#[path = "trait.rs"]
pub mod traits;

pub use config::{Config, RetentionPolicy};
pub use error::Error;
pub use local::LocalRegistry;
pub use metrics::Metric;
pub use traits::{
    MetricKind, MetricQuery, MetricRecord, MetricResult, Point, Query, QueryResult, Record,
    Selection,
};

/// Selects the metrics registry implementation at runtime.
///
/// The shared recording and querying contract is [`traits::MetricsRegistry`].
#[derive(Clone, Debug)]
pub enum MetricsRegistry {
    /// Uses the local backend.
    Local(LocalRegistry),
}

impl traits::MetricsRegistry for MetricsRegistry {
    async fn record(&self, records: &[Record]) -> Result<(), Error> {
        match self {
            Self::Local(registry) => traits::MetricsRegistry::record(registry, records).await,
        }
    }

    async fn query(&self, queries: &[Query]) -> Result<Vec<QueryResult>, Error> {
        match self {
            Self::Local(registry) => traits::MetricsRegistry::query(registry, queries).await,
        }
    }
}

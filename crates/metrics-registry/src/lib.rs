//! Batch recording and querying interfaces over a namespace of keys.
//!
//! [`MetricsRegistry`] accepts absolute gauge values and counter increments through
//! [`Record`] batches, and reads current or historical values through [`Query`]
//! batches. Implementations own persistence, system time, caches and retention.
//! [`LocalRegistry::new`] accepts an already opened RocksDB database and a shared
//! retention [`Config`]. Use [`LocalRegistry::column_families`] when opening the
//! database to install the required native merge operators. Counters use native
//! additions; gauges coordinate read-modify-write operations by source key.
//! Recording maintains requested caches and invalidates other affected caches.
//! Queries use one snapshot and reconstruct their results when caches are invalid.
//! Historical retention cleanup is not implemented yet.
//! Metric identifiers may use the built-in [`Metric`] enum or any type implementing
//! `Into<u16>`. Recording and querying consume their batches to convert identifiers
//! without requiring metric types to implement `Copy`, `Clone`, `Send` or `Sync`.
//!
//! # Examples
//!
//! Record stored bytes and a request count for one key, then query the stored-byte
//! current and request history through the registry enum.
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
//!     let records = vec![Record {
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
//!     registry.record(records).await?;
//!
//!     let end = SystemTime::now();
//!     let start = end
//!         .checked_sub(Duration::from_secs(3600))
//!         .ok_or(Error::Overflow)?;
//!     let queries = vec![Query {
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
//!     registry.query(queries).await
//! }
//! ```

#[cfg(feature = "cli")]
pub mod cli;
pub mod config;
pub mod error;
#[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
pub mod grpc;
pub mod local;
pub mod metrics;
#[cfg(feature = "grpc-client")]
pub mod remote;
#[path = "trait.rs"]
pub mod traits;

pub use config::{Config, RetentionPolicy};
pub use error::Error;
#[cfg(feature = "grpc-server")]
pub use grpc::GrpcService;
pub use local::LocalRegistry;
pub use metrics::Metric;
#[cfg(feature = "grpc-client")]
pub use remote::RemoteRegistry;
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
    /// Uses a connected gRPC registry.
    #[cfg(feature = "grpc-client")]
    Remote(RemoteRegistry),
}

impl traits::MetricsRegistry for MetricsRegistry {
    fn record<Metric: Into<u16>>(
        &self,
        records: Vec<Record<Metric>>,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        let records: Vec<Record<u16>> = records.into_iter().map(Record::into_ids).collect();
        async move {
            match self {
                Self::Local(registry) => traits::MetricsRegistry::record(registry, records).await,
                #[cfg(feature = "grpc-client")]
                Self::Remote(registry) => traits::MetricsRegistry::record(registry, records).await,
            }
        }
    }

    fn query<Metric: Into<u16>>(
        &self,
        queries: Vec<Query<Metric>>,
    ) -> impl std::future::Future<Output = Result<Vec<QueryResult>, Error>> + Send {
        let queries: Vec<Query<u16>> = queries.into_iter().map(Query::into_ids).collect();
        async move {
            match self {
                Self::Local(registry) => traits::MetricsRegistry::query(registry, queries).await,
                #[cfg(feature = "grpc-client")]
                Self::Remote(registry) => traits::MetricsRegistry::query(registry, queries).await,
            }
        }
    }
}

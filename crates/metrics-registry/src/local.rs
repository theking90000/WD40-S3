//! Local metrics registry backend.

use std::sync::{Arc, Mutex, RwLock};

use rocksdb::{ColumnFamilyDescriptor, DB, Options};

use crate::{Config, Error, Query, QueryResult, Record, traits::MetricsRegistry};

mod aggregate;
mod codec;
mod merge;
mod read;
mod record;
mod storage;

use aggregate::Catalog;
use codec::Store;
use storage::Storage;

const GAUGE_LOCKS: usize = 256;

/// Shared handle to a caller-opened RocksDB database.
///
/// Clones share the database, configuration, gauge locks and cache catalog.
/// Counter writes use native additions. Gauge read-modify-write sequences use
/// bounded, shared key locks. Unrelated gauge keys may share a lock by hash.
/// Ordinary writers share access to the catalog; cold cache reconstruction takes
/// exclusive access until its publication batch commits. Queries use snapshots
/// without acquiring these locks. Blocking storage calls run on a worker pool.
/// A cold reconstruction temporarily blocks all writers; warm cache additions
/// do not take an exclusive aggregate lock.
/// Historical retention cleanup is not implemented yet.
#[derive(Clone, Debug)]
pub struct LocalRegistry {
    storage: Arc<Storage>,
    config: Arc<Config>,
    gauges: Arc<[Mutex<()>; GAUGE_LOCKS]>,
    caches: Arc<RwLock<Catalog>>,
}

impl LocalRegistry {
    /// Builds the three column family descriptors with the required native merge
    /// operators, preserving caller-supplied tuning options.
    ///
    /// Pass these descriptors to [`DB::open_cf_descriptors`] when opening or
    /// reopening the database. Merge operators must be installed before `new`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{error::Error, path::Path, time::Duration};
    /// use metrics_registry::{Config, LocalRegistry, RetentionPolicy};
    /// use rocksdb::{DB, DBCompressionType, Options};
    ///
    /// fn open(path: &Path) -> Result<LocalRegistry, Box<dyn Error + Send + Sync>> {
    ///     let mut options = Options::default();
    ///     options.create_if_missing(true);
    ///     options.create_missing_column_families(true);
    ///     options.set_compression_type(DBCompressionType::None);
    ///     let database = DB::open_cf_descriptors(
    ///         &options, path, LocalRegistry::column_families(&options),
    ///     )?;
    ///     Ok(LocalRegistry::new(database, Config {
    ///         policies: vec![RetentionPolicy {
    ///             resolution: Duration::from_secs(300),
    ///             retention: Duration::from_secs(86_400),
    ///         }],
    ///     })?)
    /// }
    /// ```
    pub fn column_families(options: &Options) -> [ColumnFamilyDescriptor; 3] {
        Store::ALL.map(|store| {
            let mut options = options.clone();
            merge::configure(store, &mut options);
            ColumnFamilyDescriptor::new(store.name(), options)
        })
    }

    /// Validates shared retention policies and retains an initialized database.
    ///
    /// The caller must open the `current`, `history` and `aggregates` column
    /// families using [`Self::column_families`]. This constructor does not open,
    /// configure or write to the database. Configuration is retained in memory
    /// and applies to every key and metric. The binding cannot inspect installed
    /// merge callbacks; databases opened with other descriptors are unsupported.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`] if policies are empty, resolutions are
    /// zero, fractional or duplicated, or a retention is shorter than its
    /// resolution. Returns [`Error::MissingColumnFamily`] if a required column
    /// family is unavailable. Returns [`Error::CorruptData`] or [`Error::Backend`]
    /// if the existing cache catalog cannot be read.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use metrics_registry::{Config, Error, LocalRegistry, RetentionPolicy};
    ///
    /// fn attach(database: rocksdb::DB) -> Result<LocalRegistry, Error> {
    ///     LocalRegistry::new(database, Config {
    ///         policies: vec![RetentionPolicy {
    ///             resolution: Duration::from_secs(300),
    ///             retention: Duration::from_secs(86_400),
    ///         }],
    ///     })
    /// }
    /// ```
    pub fn new(database: DB, config: Config) -> Result<Self, Error> {
        config.validate()?;
        let storage = Storage::new(database)?;
        let caches = aggregate::catalog(&storage.snapshot())?;
        Ok(Self {
            storage: Arc::new(storage),
            config: Arc::new(config),
            gauges: Arc::new(std::array::from_fn(|_| Mutex::new(()))),
            caches: Arc::new(RwLock::new(caches)),
        })
    }
}

impl MetricsRegistry for LocalRegistry {
    fn record<Metric: Into<u16>>(
        &self,
        records: Vec<Record<Metric>>,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        let updates = record::prepare(records);
        let registry = self.clone();
        async move {
            let updates = updates?;
            if updates.is_empty() {
                return Ok(());
            }
            blocking::unblock(move || record::apply(&registry, updates)).await
        }
    }

    fn query<Metric: Into<u16>>(
        &self,
        queries: Vec<Query<Metric>>,
    ) -> impl std::future::Future<Output = Result<Vec<QueryResult>, Error>> + Send {
        let queries: Vec<Query<u16>> = queries.into_iter().map(Query::into_ids).collect();
        let registry = self.clone();
        async move {
            if queries.is_empty() {
                return Ok(Vec::new());
            }
            blocking::unblock(move || read::query(&registry, &queries)).await
        }
    }
}

//! Local metrics registry backend.

use std::sync::Arc;

use rocksdb::DB;

use crate::{Config, Error, Query, QueryResult, Record, traits::MetricsRegistry};

mod codec;
mod storage;

use storage::Storage;

/// Shared handle to a caller-opened RocksDB database.
///
/// Clones retain the same database and configuration. Both registry operations return
/// [`Error::LocalUnavailable`] until local storage is implemented.
#[derive(Clone, Debug)]
pub struct LocalRegistry {
    _storage: Arc<Storage>,
    _config: Arc<Config>,
}

impl LocalRegistry {
    /// Validates shared retention policies and retains an initialized database.
    ///
    /// The caller must open the `current`, `history` and `aggregates` column
    /// families. This constructor does not open, configure or write to the
    /// database. Configuration is retained in memory and applies to every key
    /// and metric.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidConfig`] if policies are empty, resolutions are
    /// zero, fractional or duplicated, or a retention is shorter than its
    /// resolution. Returns [`Error::MissingColumnFamily`] if a required column
    /// family is unavailable.
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
        Ok(Self {
            _storage: Arc::new(Storage::new(database)?),
            _config: Arc::new(config),
        })
    }
}

impl MetricsRegistry for LocalRegistry {
    async fn record(&self, _records: &[Record]) -> Result<(), Error> {
        Err(Error::LocalUnavailable)
    }

    async fn query(&self, _queries: &[Query]) -> Result<Vec<QueryResult>, Error> {
        Err(Error::LocalUnavailable)
    }
}

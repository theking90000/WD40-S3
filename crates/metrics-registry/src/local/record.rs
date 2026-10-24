//! Atomic source writes and native aggregate additions.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rocksdb::WriteBatch;

use crate::{Config, Error, MetricKind, Record};

use super::{
    LocalRegistry,
    aggregate::{self, CacheId, Catalog},
    codec::{CacheEntry, Key, Series},
    merge::{self, Operand},
    read::floor,
};

#[derive(Default)]
pub(super) struct Prepared {
    updates: BTreeMap<(u16, MetricKind, String), i128>,
    requested: BTreeSet<(u16, MetricKind, String)>,
}

impl Prepared {
    pub(super) fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    fn requests(&self, config: &Config) -> BTreeSet<CacheId> {
        let mut requests = BTreeSet::new();
        for (metric, kind, prefix) in &self.requested {
            if *kind == MetricKind::Gauge {
                requests.insert(CacheId {
                    prefix: prefix.clone(),
                    metric: *metric,
                    kind: *kind,
                    resolution: None,
                });
            }
            for policy in &config.policies {
                requests.insert(CacheId {
                    prefix: prefix.clone(),
                    metric: *metric,
                    kind: *kind,
                    resolution: Some(policy.resolution.as_secs()),
                });
            }
        }
        requests
    }
}

/// Validate all input before grouping it or accessing the database.
pub(super) fn prepare<Metric: Into<u16>>(records: Vec<Record<Metric>>) -> Result<Prepared, Error> {
    for record in &records {
        if record.key.is_empty() {
            return Err(Error::InvalidRecord("keys must not be empty".into()));
        }
        for prefix in &record.aggregates {
            if !record.key.starts_with(prefix) {
                return Err(Error::InvalidRecord(
                    "aggregates must be key prefixes".into(),
                ));
            }
        }
        for metric in &record.metrics {
            if metric.kind == MetricKind::Counter && metric.value < 0 {
                return Err(Error::InvalidRecord(
                    "counter increments must not be negative".into(),
                ));
            }
        }
    }
    let mut prepared = Prepared::default();
    for record in records {
        for metric in record.metrics {
            if metric.value == 0 {
                continue;
            }
            let metric_id = metric.metric.into();
            let update = prepared
                .updates
                .entry((metric_id, metric.kind, record.key.clone()))
                .or_default();
            *update = update
                .checked_add(i128::from(metric.value))
                .ok_or(Error::Overflow)?;
            for prefix in &record.aggregates {
                prepared
                    .requested
                    .insert((metric_id, metric.kind, prefix.clone()));
            }
        }
    }
    prepared.updates.retain(|_, delta| *delta != 0);
    Ok(prepared)
}

/// Normal writers share the catalog gate. Only cold cache reconstruction takes
/// it exclusively, including discovery of new source keys under a prefix.
pub(super) fn apply(registry: &LocalRegistry, prepared: Prepared) -> Result<(), Error> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Backend(Box::new(error)))?;
    let requested = prepared.requests(&registry.config);
    let catalog = registry.caches.read().map_err(|_| merge::poisoned())?;
    let view = registry.storage.snapshot();
    let mut cold = false;
    for id in &requested {
        if !id.valid(&view)? {
            cold = true;
            break;
        }
    }
    if !cold {
        return commit(
            registry,
            &prepared,
            &catalog,
            &requested,
            elapsed,
            WriteBatch::default(),
        );
    }

    // Never upgrade a held read lock. Start from a fresh snapshot after acquiring
    // the exclusive gate, then recompute source values and cache bases.
    drop(view);
    drop(catalog);
    let mut catalog = registry.caches.write().map_err(|_| merge::poisoned())?;
    let view = registry.storage.snapshot();
    let mut batch = WriteBatch::default();
    for id in &requested {
        if !id.valid(&view)? {
            let rebuilt = aggregate::reconstruct(&view, id)?;
            aggregate::publish(&registry.storage, &mut batch, id, rebuilt)?;
        }
        // Register before the write: an ambiguous I/O error may still have
        // committed the cache. Subsequent writers must know it can exist.
        aggregate::register(&mut catalog, id);
    }
    commit(registry, &prepared, &catalog, &requested, elapsed, batch)
}

fn commit(
    registry: &LocalRegistry,
    prepared: &Prepared,
    catalog: &Catalog,
    requested: &BTreeSet<CacheId>,
    elapsed: Duration,
    mut batch: WriteBatch,
) -> Result<(), Error> {
    let windows: Vec<(u64, u64)> = registry
        .config
        .policies
        .iter()
        .map(|policy| {
            let resolution = policy.resolution.as_secs();
            Ok((resolution, floor(elapsed.as_secs(), resolution)?))
        })
        .collect::<Result<_, Error>>()?;
    let mut changes = BTreeMap::new();
    for ((metric, kind, key), delta) in &prepared.updates {
        let series = Series {
            metric: *metric,
            kind: *kind,
            key,
        };
        if *kind == MetricKind::Gauge {
            registry
                .storage
                .merge(&mut batch, Key::Current(series), Operand::Add(*delta))?;
        }
        for &(resolution, start) in &windows {
            registry.storage.merge(
                &mut batch,
                Key::Window {
                    resolution,
                    start,
                    series,
                },
                Operand::Add(*delta),
            )?;
        }
        aggregate::contributions(catalog, series, *delta, &mut changes)?;
    }
    for (id, delta) in changes {
        if delta == 0 {
            continue;
        }
        if !requested.contains(&id) {
            registry
                .storage
                .merge(&mut batch, id.status_key(), Operand::Invalidate)?;
            continue;
        }
        let key = match id.resolution {
            None => id.status_key(),
            Some(resolution) => Key::AggregateHistory {
                resolution,
                series: id.series(),
                entry: CacheEntry::Window(floor(elapsed.as_secs(), resolution)?),
            },
        };
        registry
            .storage
            .merge(&mut batch, key, Operand::Add(delta))?;
    }
    if !batch.is_empty() {
        registry.storage.write(batch)?;
    }
    Ok(())
}

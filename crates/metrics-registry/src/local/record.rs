//! Atomic source writes and native aggregate additions.

use std::{
    collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::MutexGuard,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rocksdb::WriteBatch;

use crate::{Config, Error, MetricKind, Record};

use super::{
    GAUGE_LOCKS, LocalRegistry,
    aggregate::{self, CacheId, Catalog},
    codec::{CacheEntry, GaugeValue, Key, Series, Value},
    merge::{self, Operand},
    read::floor,
    storage::View,
};

#[derive(Default)]
pub(super) struct Prepared {
    updates: BTreeMap<(u16, String), Update>,
    requested: BTreeSet<(u16, MetricKind, String)>,
}

#[derive(Default)]
struct Update {
    gauge: Option<i64>,
    counter: i128,
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
            return Err(Error::InvalidRecord("keys must not be empty"));
        }
        for prefix in &record.aggregates {
            if !record.key.starts_with(prefix) {
                return Err(Error::InvalidRecord("aggregates must be key prefixes"));
            }
        }
        for metric in &record.metrics {
            if metric.kind == MetricKind::Counter && metric.value < 0 {
                return Err(Error::InvalidRecord(
                    "counter increments must not be negative",
                ));
            }
        }
    }
    let mut prepared = Prepared::default();
    for record in records {
        for metric in record.metrics {
            if metric.kind == MetricKind::Counter && metric.value == 0 {
                continue;
            }
            let metric_id = metric.metric.into();
            let update = prepared
                .updates
                .entry((metric_id, record.key.clone()))
                .or_default();
            match metric.kind {
                MetricKind::Gauge => update.gauge = Some(metric.value),
                MetricKind::Counter => {
                    update.counter = update
                        .counter
                        .checked_add(i128::from(metric.value))
                        .ok_or(Error::Overflow)?;
                }
            }
            for prefix in &record.aggregates {
                prepared
                    .requested
                    .insert((metric_id, metric.kind, prefix.clone()));
            }
        }
    }
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
    let guards = lock_gauges(registry, &prepared)?;
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
            &view,
            &requested,
            elapsed,
            WriteBatch::default(),
        );
    }

    // Never upgrade a held read lock. Start from a fresh snapshot after acquiring
    // the exclusive gate, then recompute source values and cache bases.
    drop(view);
    drop(guards);
    drop(catalog);
    let mut catalog = registry.caches.write().map_err(|_| merge::poisoned())?;
    let _guards = lock_gauges(registry, &prepared)?;
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
    commit(
        registry, &prepared, &catalog, &view, &requested, elapsed, batch,
    )
}

fn lock_gauges<'a>(
    registry: &'a LocalRegistry,
    prepared: &Prepared,
) -> Result<Vec<MutexGuard<'a, ()>>, Error> {
    let mut indices = BTreeSet::new();
    for ((_, key), update) in &prepared.updates {
        if update.gauge.is_some() {
            let mut hasher = DefaultHasher::new();
            key.hash(&mut hasher);
            let index = hasher
                .finish()
                .checked_rem(GAUGE_LOCKS as u64)
                .ok_or(Error::Overflow)?;
            indices.insert(usize::try_from(index).map_err(|_| Error::Overflow)?);
        }
    }
    let mut guards = Vec::new();
    for index in indices {
        let lock = registry
            .gauges
            .get(index)
            .ok_or(Error::CorruptData("invalid gauge lock index"))?;
        guards.push(lock.lock().map_err(|_| merge::poisoned())?);
    }
    Ok(guards)
}

fn commit(
    registry: &LocalRegistry,
    prepared: &Prepared,
    catalog: &Catalog,
    view: &View<'_>,
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
    for ((metric, key), update) in &prepared.updates {
        if let Some(value) = update.gauge {
            let series = Series {
                metric: *metric,
                kind: MetricKind::Gauge,
                key,
            };
            let previous = match view.get(Key::Current(series))? {
                Some(Value::Gauge(value)) => Some(value),
                None => None,
                _ => return Err(Error::CorruptData("invalid current gauge value")),
            };
            let timestamp = u64::try_from(elapsed.as_nanos()).map_err(|_| Error::Overflow)?;
            if let Some((gauge, delta)) = assignment(previous, value, timestamp)? {
                registry
                    .storage
                    .put(&mut batch, Key::Current(series), Value::Gauge(gauge))?;
                for &(resolution, start) in &windows {
                    registry.storage.put(
                        &mut batch,
                        Key::Window {
                            resolution,
                            start,
                            series,
                        },
                        Value::Gauge(gauge),
                    )?;
                }
                aggregate::contributions(catalog, series, delta, &mut changes)?;
            }
        }
        if update.counter != 0 {
            let series = Series {
                metric: *metric,
                kind: MetricKind::Counter,
                key,
            };
            for &(resolution, start) in &windows {
                registry.storage.merge(
                    &mut batch,
                    Key::Window {
                        resolution,
                        start,
                        series,
                    },
                    Operand::Add(update.counter),
                )?;
            }
            aggregate::contributions(catalog, series, update.counter, &mut changes)?;
        }
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

fn assignment(
    previous: Option<GaugeValue>,
    value: i64,
    timestamp: u64,
) -> Result<Option<(GaugeValue, i128)>, Error> {
    let sequence = match previous {
        Some(previous) if previous.timestamp > timestamp => return Ok(None),
        Some(previous) if previous.timestamp == timestamp => {
            previous.sequence.checked_add(1).ok_or(Error::Overflow)?
        }
        _ => 0,
    };
    let old_value = previous.map_or(0, |previous| previous.value);
    let delta = i128::from(value)
        .checked_sub(i128::from(old_value))
        .ok_or(Error::Overflow)?;
    Ok(Some((
        GaugeValue {
            value,
            timestamp,
            sequence,
        },
        delta,
    )))
}

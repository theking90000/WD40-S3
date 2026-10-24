//! Aggregate identities, snapshot reconstruction and cache publication.

use std::collections::{BTreeMap, BTreeSet};

use rocksdb::WriteBatch;

use crate::{Error, MetricKind};

use super::{
    codec::{
        CacheEntry, Key, Series, Store, Value, aggregate_history_prefix, prefix_end,
        resolution_prefix,
    },
    storage::{Storage, View},
};

pub(super) type Catalog = BTreeMap<String, BTreeSet<(u16, MetricKind, Option<u64>)>>;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct CacheId {
    pub prefix: String,
    pub metric: u16,
    pub kind: MetricKind,
    pub resolution: Option<u64>,
}

pub(super) enum Rebuilt {
    Current(i128),
    History {
        reference: i128,
        windows: BTreeMap<u64, i128>,
    },
}

impl CacheId {
    pub(super) fn series(&self) -> Series<'_> {
        Series {
            metric: self.metric,
            kind: self.kind,
            key: &self.prefix,
        }
    }

    pub(super) fn status_key(&self) -> Key<'_> {
        match self.resolution {
            None => Key::AggregateCurrent(self.series()),
            Some(resolution) => Key::AggregateHistory {
                resolution,
                series: self.series(),
                entry: CacheEntry::Valid,
            },
        }
    }

    pub(super) fn valid(&self, view: &View<'_>) -> Result<bool, Error> {
        Ok(matches!(
            view.get(self.status_key())?,
            Some(Value::Cache(Some(_))) | Some(Value::Valid(true))
        ))
    }
}

pub(super) fn register(catalog: &mut Catalog, id: &CacheId) {
    catalog
        .entry(id.prefix.clone())
        .or_default()
        .insert((id.metric, id.kind, id.resolution));
}

pub(super) fn catalog(view: &View<'_>) -> Result<Catalog, Error> {
    let mut catalog = Catalog::new();
    view.scan_prefix(Store::Aggregates, b"", |key, _| {
        let (series, resolution) = match key {
            Key::AggregateCurrent(series) => (series, None),
            Key::AggregateHistory {
                series,
                resolution,
                entry: CacheEntry::Valid,
            } => (series, Some(resolution)),
            _ => return Ok(()),
        };
        register(
            &mut catalog,
            &CacheId {
                prefix: series.key.to_owned(),
                metric: series.metric,
                kind: series.kind,
                resolution,
            },
        );
        Ok(())
    })?;
    Ok(catalog)
}

/// Find only existing ancestor caches, using the in-memory catalog rather than
/// issuing a database read for every possible string prefix.
pub(super) fn contributions(
    catalog: &Catalog,
    series: Series<'_>,
    delta: i128,
    changes: &mut BTreeMap<CacheId, i128>,
) -> Result<(), Error> {
    if delta == 0 {
        return Ok(());
    }
    for end in series
        .key
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(series.key.len()))
    {
        let prefix = series
            .key
            .get(..end)
            .ok_or(Error::CorruptData("invalid source prefix boundary"))?;
        let Some(entries) = catalog.get(prefix) else {
            continue;
        };
        for &(metric, kind, resolution) in entries {
            if metric != series.metric || kind != series.kind {
                continue;
            }
            let id = CacheId {
                prefix: prefix.to_owned(),
                metric,
                kind,
                resolution,
            };
            let total = changes.entry(id).or_insert(0);
            *total = total.checked_add(delta).ok_or(Error::Overflow)?;
        }
    }
    Ok(())
}

pub(super) fn load(view: &View<'_>, id: &CacheId) -> Result<Option<Rebuilt>, Error> {
    match (id.resolution, view.get(id.status_key())?) {
        (None, Some(Value::Cache(Some(total)))) => return Ok(Some(Rebuilt::Current(total))),
        (None, None | Some(Value::Cache(None) | Value::Overflow)) => return Ok(None),
        (Some(_), None | Some(Value::Valid(false))) => return Ok(None),
        (Some(_), Some(Value::Valid(true))) => {}
        _ => return Err(Error::CorruptData("invalid aggregate status")),
    }
    let resolution = id
        .resolution
        .ok_or(Error::CorruptData("missing cache resolution"))?;
    let reference = if id.kind == MetricKind::Gauge {
        total(view.get(Key::AggregateHistory {
            resolution,
            series: id.series(),
            entry: CacheEntry::Reference,
        })?)?
    } else {
        0
    };
    let mut prefix = aggregate_history_prefix(id.series(), resolution)?;
    prefix.push(2);
    let mut windows = BTreeMap::new();
    view.scan_prefix(Store::Aggregates, &prefix, |key, value| {
        let Key::AggregateHistory {
            entry: CacheEntry::Window(start),
            ..
        } = key
        else {
            return Err(Error::CorruptData("invalid aggregate window"));
        };
        windows.insert(start, total(Some(value))?);
        Ok(())
    })?;
    Ok(Some(Rebuilt::History { reference, windows }))
}

pub(super) fn reconstruct(view: &View<'_>, id: &CacheId) -> Result<Rebuilt, Error> {
    let Some(resolution) = id.resolution else {
        let mut sum = 0_i128;
        let prefix = Key::Current(id.series()).encode()?;
        view.scan_prefix(Store::Current, &prefix, |_, value| {
            sum = sum
                .checked_add(total(Some(value))?)
                .ok_or(Error::Overflow)?;
            Ok(())
        })?;
        return Ok(Rebuilt::Current(sum));
    };
    let mut reference = 0_i128;
    if id.kind == MetricKind::Gauge {
        let prefix = Key::Reference {
            resolution,
            series: id.series(),
        }
        .encode()?;
        view.scan_prefix(Store::History, &prefix, |_, value| {
            reference = reference
                .checked_add(total(Some(value))?)
                .ok_or(Error::Overflow)?;
            Ok(())
        })?;
    }
    let mut windows = BTreeMap::new();
    view.scan_prefix(
        Store::History,
        &resolution_prefix(resolution)?,
        |key, value| {
            let Key::Window { start, series, .. } = key else {
                return Err(Error::CorruptData("invalid source window"));
            };
            if series.metric != id.metric
                || series.kind != id.kind
                || !series.key.starts_with(&id.prefix)
            {
                return Ok(());
            }
            let delta = total(Some(value))?;
            let sum = windows.entry(start).or_insert(0_i128);
            *sum = sum.checked_add(delta).ok_or(Error::Overflow)?;
            Ok(())
        },
    )?;
    Ok(Rebuilt::History { reference, windows })
}

/// Caller holds the exclusive cache gate, preventing writes and new source keys
/// between reconstruction's snapshot and the atomic publication batch.
pub(super) fn publish(
    storage: &Storage,
    batch: &mut WriteBatch,
    id: &CacheId,
    rebuilt: Rebuilt,
) -> Result<(), Error> {
    match (id.resolution, rebuilt) {
        (None, Rebuilt::Current(total)) => {
            storage.put(batch, id.status_key(), Value::Cache(Some(total)))?;
        }
        (Some(resolution), Rebuilt::History { reference, windows }) => {
            let prefix = aggregate_history_prefix(id.series(), resolution)?;
            let end = prefix_end(&prefix).ok_or(Error::Overflow)?;
            storage.delete_range(batch, Store::Aggregates, &prefix, &end)?;
            if id.kind == MetricKind::Gauge {
                storage.put(
                    batch,
                    Key::AggregateHistory {
                        resolution,
                        series: id.series(),
                        entry: CacheEntry::Reference,
                    },
                    Value::Total(reference),
                )?;
            }
            for (start, value) in windows {
                if value != 0 {
                    storage.put(
                        batch,
                        Key::AggregateHistory {
                            resolution,
                            series: id.series(),
                            entry: CacheEntry::Window(start),
                        },
                        Value::Total(value),
                    )?;
                }
            }
            storage.put(batch, id.status_key(), Value::Valid(true))?;
        }
        _ => return Err(Error::CorruptData("aggregate selection mismatch")),
    }
    Ok(())
}

pub(super) fn total(value: Option<Value>) -> Result<i128, Error> {
    match value {
        None => Ok(0),
        Some(Value::Total(total)) => Ok(total),
        Some(Value::Overflow) => Err(Error::Overflow),
        _ => Err(Error::CorruptData("invalid numeric total")),
    }
}

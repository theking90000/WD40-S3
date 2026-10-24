//! Read-only queries over one snapshot, using valid caches or source reconstruction.

use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{Error, MetricKind, MetricResult, Point, Query, QueryResult, Selection};

use super::{
    LocalRegistry,
    aggregate::{self, CacheId, Rebuilt},
};

pub(super) fn query(
    registry: &LocalRegistry,
    queries: &[Query],
) -> Result<Vec<QueryResult>, Error> {
    let view = registry.storage.snapshot();
    let now = SystemTime::now();
    let elapsed = now
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Backend(Box::new(error)))?;
    for query in queries {
        for metric in &query.metrics {
            match &metric.selection {
                Selection::Current if metric.kind == MetricKind::Counter => {
                    return Err(Error::InvalidQuery("counters have no current selection"));
                }
                Selection::Current => {}
                Selection::History { resolution, range } => {
                    if resolution.is_zero() || resolution.subsec_nanos() != 0 {
                        return Err(Error::InvalidQuery(
                            "resolution must be nonzero whole seconds",
                        ));
                    }
                    let policy = registry
                        .config
                        .policies
                        .iter()
                        .find(|policy| policy.resolution == *resolution)
                        .ok_or(Error::ResolutionUnavailable(*resolution))?;
                    if range.start > range.end {
                        return Err(Error::InvalidQuery("history range is reversed"));
                    }
                    let start = epoch(range.start)?;
                    epoch(range.end)?;
                    let cutoff = floor(
                        elapsed.saturating_sub(policy.retention).as_secs(),
                        resolution.as_secs(),
                    )?;
                    if range.start != range.end && start < Duration::from_secs(cutoff) {
                        return Err(Error::HistoryExpired);
                    }
                }
            }
        }
    }
    let mut results = Vec::new();
    for query in queries {
        let prefixes = prefixes(&query.prefixes);
        let mut metrics = Vec::new();
        for metric in &query.metrics {
            let resolution = match &metric.selection {
                Selection::Current => None,
                Selection::History { resolution, .. } => Some(resolution.as_secs()),
            };
            let mut current = 0_i128;
            let mut reference = 0_i128;
            let mut windows = BTreeMap::new();
            for prefix in &prefixes {
                let id = CacheId {
                    prefix: (*prefix).to_owned(),
                    metric: metric.metric,
                    kind: metric.kind,
                    resolution,
                };
                let rebuilt = match aggregate::load(&view, &id)? {
                    Some(cached) => cached,
                    None => aggregate::reconstruct(&view, &id)?,
                };
                match rebuilt {
                    Rebuilt::Current(total) => {
                        current = current.checked_add(total).ok_or(Error::Overflow)?
                    }
                    Rebuilt::History {
                        reference: base,
                        windows: values,
                    } => {
                        reference = reference.checked_add(base).ok_or(Error::Overflow)?;
                        for (start, value) in values {
                            let sum = windows.entry(start).or_insert(0_i128);
                            *sum = sum.checked_add(value).ok_or(Error::Overflow)?;
                        }
                    }
                }
            }
            metrics.push(match &metric.selection {
                Selection::Current => {
                    MetricResult::Current(i64::try_from(current).map_err(|_| Error::Overflow)?)
                }
                Selection::History { resolution, range } => MetricResult::History(points(
                    metric.kind,
                    resolution.as_secs(),
                    epoch(range.start)?,
                    epoch(range.end)?.min(elapsed),
                    elapsed,
                    reference,
                    &windows,
                )?),
            });
        }
        results.push(QueryResult { metrics });
    }
    Ok(results)
}

fn prefixes(values: &[String]) -> Vec<&str> {
    let mut values: Vec<&str> = values.iter().map(String::as_str).collect();
    values.sort_unstable();
    let mut result: Vec<&str> = Vec::new();
    for prefix in values {
        if !result
            .last()
            .is_some_and(|ancestor| prefix.starts_with(*ancestor))
        {
            result.push(prefix);
        }
    }
    result
}

fn points(
    kind: MetricKind,
    resolution: u64,
    start: Duration,
    end: Duration,
    now: Duration,
    reference: i128,
    windows: &BTreeMap<u64, i128>,
) -> Result<Vec<Point>, Error> {
    if start >= end {
        return Ok(Vec::new());
    }
    let mut window = floor(start.as_secs(), resolution)?;
    let mut gauge = reference;
    if kind == MetricKind::Gauge {
        for (_, delta) in windows.range(..window) {
            gauge = gauge.checked_add(*delta).ok_or(Error::Overflow)?;
        }
    }
    let mut result = Vec::new();
    while Duration::from_secs(window) < end {
        let value = windows.get(&window).copied().map_or(0, |value| value);
        let value = if kind == MetricKind::Gauge {
            gauge = gauge.checked_add(value).ok_or(Error::Overflow)?;
            gauge
        } else {
            value
        };
        let next = window.checked_add(resolution).ok_or(Error::Overflow)?;
        result.push(Point {
            start: UNIX_EPOCH
                .checked_add(Duration::from_secs(window))
                .ok_or(Error::Overflow)?,
            value: i64::try_from(value).map_err(|_| Error::Overflow)?,
            complete: Duration::from_secs(next) <= now,
        });
        window = next;
    }
    Ok(result)
}

pub(super) fn floor(seconds: u64, resolution: u64) -> Result<u64, Error> {
    let remainder = seconds.checked_rem(resolution).ok_or(Error::Overflow)?;
    seconds.checked_sub(remainder).ok_or(Error::Overflow)
}

fn epoch(time: SystemTime) -> Result<Duration, Error> {
    time.duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InvalidQuery("history times must not precede the Unix epoch"))
}

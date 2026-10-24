//! Private binary storage format. Numeric key fields are big-endian.
//!
//! Current keys contain metric, kind and the remaining UTF-8 key bytes.
//! History windows contain tag 0, resolution seconds, window-start seconds,
//! metric, kind and key. References use tag 1 and omit the window start.
//! Aggregate keys contain a length-prefixed UTF-8 prefix, metric, kind and
//! selection. Historical selections also contain resolution seconds and an
//! entry tag. Resolution values are stored directly, without a dictionary.

use crate::{Error, Metric, MetricKind};

#[derive(Clone, Copy, Debug)]
pub(super) enum Store {
    Current,
    History,
    Aggregates,
}

impl Store {
    pub(super) const ALL: [Self; 3] = [Self::Current, Self::History, Self::Aggregates];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::History => "history",
            Self::Aggregates => "aggregates",
        }
    }
}

/// A source key, or an aggregate prefix, paired with its metric and kind.
#[derive(Clone, Copy, Debug)]
pub(super) struct Series<'a> {
    pub metric: Metric,
    pub kind: MetricKind,
    pub key: &'a str,
}

/// A history cache is readable only while its `Valid` entry exists.
#[derive(Clone, Copy, Debug)]
pub(super) enum CacheEntry {
    Valid,
    Reference,
    Window(u64),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Key<'a> {
    Current(Series<'a>),
    Window {
        resolution: u64,
        start: u64,
        series: Series<'a>,
    },
    Reference {
        resolution: u64,
        series: Series<'a>,
    },
    AggregateCurrent(Series<'a>),
    AggregateHistory {
        resolution: u64,
        series: Series<'a>,
        entry: CacheEntry,
    },
}

/// Assignment time is nanoseconds since the Unix epoch. The sequence orders
/// assignments sharing a timestamp. Neither field is exposed by the public API.
#[derive(Clone, Copy, Debug)]
pub(super) struct GaugeValue {
    pub value: i64,
    pub timestamp: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Value {
    Gauge(GaugeValue),
    Total(i128),
    Cache(Option<i128>),
    Valid(bool),
    Overflow,
}

impl Key<'_> {
    pub(super) fn store(self) -> Store {
        match self {
            Self::Current(_) => Store::Current,
            Self::Window { .. } | Self::Reference { .. } => Store::History,
            Self::AggregateCurrent(_) | Self::AggregateHistory { .. } => Store::Aggregates,
        }
    }

    /// Source keys also serve as scan prefixes when `series.key` is a prefix.
    pub(super) fn encode(self) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        match self {
            Self::Current(series) => {
                require_gauge(series.kind)?;
                encode_series(&mut bytes, series);
                bytes.extend_from_slice(series.key.as_bytes());
            }
            Self::Window {
                resolution,
                start,
                series,
            } => {
                bytes = window_prefix(resolution, start)?;
                encode_series(&mut bytes, series);
                bytes.extend_from_slice(series.key.as_bytes());
            }
            Self::Reference { resolution, series } => {
                require_resolution(resolution)?;
                require_gauge(series.kind)?;
                bytes.push(1);
                bytes.extend_from_slice(&resolution.to_be_bytes());
                encode_series(&mut bytes, series);
                bytes.extend_from_slice(series.key.as_bytes());
            }
            Self::AggregateCurrent(series) => {
                require_gauge(series.kind)?;
                encode_aggregate(&mut bytes, series)?;
                bytes.push(0);
            }
            Self::AggregateHistory {
                resolution,
                series,
                entry,
            } => {
                bytes = aggregate_history_prefix(series, resolution)?;
                match entry {
                    CacheEntry::Valid => bytes.push(0),
                    CacheEntry::Reference => {
                        require_gauge(series.kind)?;
                        bytes.push(1);
                    }
                    CacheEntry::Window(start) => {
                        require_window(resolution, start)?;
                        bytes.push(2);
                        bytes.extend_from_slice(&start.to_be_bytes());
                    }
                }
            }
        }
        Ok(bytes)
    }

    pub(super) fn encode_value(self, value: Value) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        match (self.value_format(), value) {
            (ValueFormat::Gauge, Value::Gauge(gauge)) => {
                bytes.extend_from_slice(&gauge.value.to_be_bytes());
                bytes.extend_from_slice(&gauge.timestamp.to_be_bytes());
                bytes.extend_from_slice(&gauge.sequence.to_be_bytes());
            }
            (ValueFormat::Total(kind), Value::Total(total)) => {
                require_total(kind, total)?;
                bytes.extend_from_slice(&total.to_be_bytes());
            }
            (ValueFormat::Cache, Value::Cache(Some(total))) => {
                bytes.push(1);
                bytes.extend_from_slice(&total.to_be_bytes());
            }
            (ValueFormat::Cache, Value::Cache(None)) => bytes.push(0),
            (ValueFormat::Valid, Value::Valid(valid)) => bytes.push(u8::from(valid)),
            (ValueFormat::Total(_) | ValueFormat::Cache, Value::Overflow) => bytes.push(2),
            _ => return Err(Error::CorruptData("value does not match its storage key")),
        }
        Ok(bytes)
    }

    pub(super) fn decode_value(self, bytes: &[u8]) -> Result<Value, Error> {
        let mut reader = Reader(bytes);
        let value = match self.value_format() {
            ValueFormat::Gauge => Value::Gauge(GaugeValue {
                value: i64::from_be_bytes(reader.take()?),
                timestamp: reader.u64()?,
                sequence: reader.u64()?,
            }),
            ValueFormat::Total(kind) => {
                if bytes == [2] {
                    return Ok(Value::Overflow);
                }
                // Source counters written before wide totals used eight bytes.
                let total = if bytes.len() == 8 {
                    i128::from(i64::from_be_bytes(reader.take()?))
                } else {
                    i128::from_be_bytes(reader.take()?)
                };
                require_total(kind, total)?;
                Value::Total(total)
            }
            ValueFormat::Cache => match reader.byte()? {
                0 => Value::Cache(None),
                1 => Value::Cache(Some(i128::from_be_bytes(reader.take()?))),
                2 => Value::Overflow,
                _ => return Err(Error::CorruptData("unknown cache state")),
            },
            ValueFormat::Valid => match reader.byte()? {
                0 => Value::Valid(false),
                1 => Value::Valid(true),
                _ => return Err(Error::CorruptData("unknown history cache state")),
            },
        };
        reader.finish()?;
        Ok(value)
    }

    fn value_format(self) -> ValueFormat {
        match self {
            Self::Current(_) | Self::Reference { .. } => ValueFormat::Gauge,
            Self::Window { series, .. } => match series.kind {
                MetricKind::Gauge => ValueFormat::Gauge,
                MetricKind::Counter => ValueFormat::Total(MetricKind::Counter),
            },
            Self::AggregateHistory {
                entry: CacheEntry::Valid,
                ..
            } => ValueFormat::Valid,
            Self::AggregateCurrent(_) => ValueFormat::Cache,
            Self::AggregateHistory { series, .. } => ValueFormat::Total(series.kind),
        }
    }
}

impl<'a> Key<'a> {
    pub(super) fn decode(store: Store, bytes: &'a [u8]) -> Result<Self, Error> {
        let mut reader = Reader(bytes);
        let key = match store {
            Store::Current => {
                let (metric, kind) = reader.series()?;
                require_gauge(kind)?;
                Self::Current(Series {
                    metric,
                    kind,
                    key: reader.source_key()?,
                })
            }
            Store::History => {
                let tag = reader.byte()?;
                let resolution = reader.u64()?;
                require_resolution(resolution)?;
                let start = match tag {
                    0 => Some(reader.u64()?),
                    1 => None,
                    _ => return Err(Error::CorruptData("unknown history key tag")),
                };
                let (metric, kind) = reader.series()?;
                let series = Series {
                    metric,
                    kind,
                    key: reader.source_key()?,
                };
                match start {
                    Some(start) => {
                        require_window(resolution, start)?;
                        Self::Window {
                            resolution,
                            start,
                            series,
                        }
                    }
                    None => {
                        require_gauge(kind)?;
                        Self::Reference { resolution, series }
                    }
                }
            }
            Store::Aggregates => {
                let length = usize::try_from(u32::from_be_bytes(reader.take()?))
                    .map_err(|_| Error::CorruptData("aggregate prefix length is too large"))?;
                let prefix = reader.text(length)?;
                let (metric, kind) = reader.series()?;
                let series = Series {
                    metric,
                    kind,
                    key: prefix,
                };
                let key = match reader.byte()? {
                    0 => {
                        require_gauge(kind)?;
                        Self::AggregateCurrent(series)
                    }
                    1 => {
                        let resolution = reader.u64()?;
                        require_resolution(resolution)?;
                        let entry = match reader.byte()? {
                            0 => CacheEntry::Valid,
                            1 => {
                                require_gauge(kind)?;
                                CacheEntry::Reference
                            }
                            2 => {
                                let start = reader.u64()?;
                                require_window(resolution, start)?;
                                CacheEntry::Window(start)
                            }
                            _ => return Err(Error::CorruptData("unknown aggregate entry tag")),
                        };
                        Self::AggregateHistory {
                            resolution,
                            series,
                            entry,
                        }
                    }
                    _ => return Err(Error::CorruptData("unknown aggregate selection tag")),
                };
                reader.finish()?;
                key
            }
        };
        Ok(key)
    }
}

pub(super) fn resolution_prefix(resolution: u64) -> Result<Vec<u8>, Error> {
    require_resolution(resolution)?;
    let mut bytes = vec![0];
    bytes.extend_from_slice(&resolution.to_be_bytes());
    Ok(bytes)
}

pub(super) fn window_prefix(resolution: u64, start: u64) -> Result<Vec<u8>, Error> {
    require_window(resolution, start)?;
    let mut bytes = resolution_prefix(resolution)?;
    bytes.extend_from_slice(&start.to_be_bytes());
    Ok(bytes)
}

pub(super) fn aggregate_history_prefix(
    series: Series<'_>,
    resolution: u64,
) -> Result<Vec<u8>, Error> {
    require_resolution(resolution)?;
    let mut bytes = Vec::new();
    encode_aggregate(&mut bytes, series)?;
    bytes.push(1);
    bytes.extend_from_slice(&resolution.to_be_bytes());
    Ok(bytes)
}

/// Exclusive upper bound for every key starting with `prefix`.
pub(super) fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.pop() {
        if let Some(next) = last.checked_add(1) {
            end.push(next);
            return Some(end);
        }
    }
    None
}

fn encode_series(bytes: &mut Vec<u8>, series: Series<'_>) {
    bytes.extend_from_slice(&(series.metric as u16).to_be_bytes());
    bytes.push(match series.kind {
        MetricKind::Gauge => 0,
        MetricKind::Counter => 1,
    });
}

fn encode_aggregate(bytes: &mut Vec<u8>, series: Series<'_>) -> Result<(), Error> {
    let length = u32::try_from(series.key.len()).map_err(|_| Error::Overflow)?;
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(series.key.as_bytes());
    encode_series(bytes, series);
    Ok(())
}

fn require_resolution(resolution: u64) -> Result<(), Error> {
    if resolution == 0 {
        return Err(Error::CorruptData("zero history resolution"));
    }
    Ok(())
}

fn require_window(resolution: u64, start: u64) -> Result<(), Error> {
    if start.checked_rem(resolution) != Some(0) {
        return Err(Error::CorruptData("unaligned window or zero resolution"));
    }
    Ok(())
}

fn require_gauge(kind: MetricKind) -> Result<(), Error> {
    if kind != MetricKind::Gauge {
        return Err(Error::CorruptData(
            "counter has no current or reference value",
        ));
    }
    Ok(())
}

fn require_total(kind: MetricKind, total: i128) -> Result<(), Error> {
    if kind == MetricKind::Counter && total < 0 {
        return Err(Error::CorruptData("negative counter total"));
    }
    Ok(())
}

enum ValueFormat {
    Gauge,
    Total(MetricKind),
    Cache,
    Valid,
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let (head, tail) = self
            .0
            .split_first_chunk::<N>()
            .ok_or(Error::CorruptData("truncated storage entry"))?;
        self.0 = tail;
        Ok(*head)
    }

    fn byte(&mut self) -> Result<u8, Error> {
        let [byte] = self.take()?;
        Ok(byte)
    }

    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_be_bytes(self.take()?))
    }

    fn series(&mut self) -> Result<(Metric, MetricKind), Error> {
        let metric = match u16::from_be_bytes(self.take()?) {
            id if id == Metric::StoredBytes as u16 => Metric::StoredBytes,
            id if id == Metric::DownloadedBytes as u16 => Metric::DownloadedBytes,
            id if id == Metric::UploadedBytes as u16 => Metric::UploadedBytes,
            id if id == Metric::RequestCount as u16 => Metric::RequestCount,
            _ => return Err(Error::CorruptData("unknown metric identifier")),
        };
        let kind = match self.byte()? {
            0 => MetricKind::Gauge,
            1 => MetricKind::Counter,
            _ => return Err(Error::CorruptData("unknown metric kind")),
        };
        Ok((metric, kind))
    }

    fn text(&mut self, length: usize) -> Result<&'a str, Error> {
        let (head, tail) = self
            .0
            .split_at_checked(length)
            .ok_or(Error::CorruptData("truncated storage key"))?;
        self.0 = tail;
        std::str::from_utf8(head).map_err(|_| Error::CorruptData("invalid UTF-8 storage key"))
    }

    fn source_key(mut self) -> Result<&'a str, Error> {
        if self.0.is_empty() {
            return Err(Error::CorruptData("empty source key"));
        }
        self.text(self.0.len())
    }

    fn finish(self) -> Result<(), Error> {
        if !self.0.is_empty() {
            return Err(Error::CorruptData("trailing storage bytes"));
        }
        Ok(())
    }
}

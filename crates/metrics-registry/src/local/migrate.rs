//! Explicit offline conversion of absolute gauge rows to additive rows.

use std::collections::BTreeMap;

use rocksdb::{DB, IteratorMode, ReadOptions, WriteBatch, WriteOptions};

use crate::{Error, MetricKind};

use super::{
    codec::{Key, Store, Value},
    storage::{FORMAT_KEY, FORMAT_VERSION},
};

pub(super) fn v1(database: &DB) -> Result<(), Error> {
    match database.get(FORMAT_KEY).map_err(backend)?.as_deref() {
        Some(version) if version == FORMAT_VERSION => return Ok(()),
        Some(version) if version != b"1" => {
            return Err(Error::CorruptData("unsupported metrics storage version"));
        }
        _ => {}
    }
    let current = database
        .cf_handle(Store::Current.name())
        .ok_or(Error::MissingColumnFamily(Store::Current.name()))?;
    let history = database
        .cf_handle(Store::History.name())
        .ok_or(Error::MissingColumnFamily(Store::History.name()))?;
    let aggregates = database
        .cf_handle(Store::Aggregates.name())
        .ok_or(Error::MissingColumnFamily(Store::Aggregates.name()))?;
    let snapshot = database.snapshot();
    let mut batch = WriteBatch::default();

    for row in snapshot.iterator_cf_opt(&current, scan_options(), IteratorMode::Start) {
        let (bytes, value) = row.map_err(backend)?;
        let key = Key::decode(Store::Current, &bytes)?;
        batch.put_cf(
            &current,
            &bytes,
            key.encode_value(Value::Total(legacy_gauge(&value)?))?,
        );
    }

    // References are the values preceding the retained windows. Read them first,
    // although their tag sorts after window rows in the legacy format.
    let mut previous = BTreeMap::new();
    for row in snapshot.iterator_cf_opt(&history, scan_options(), IteratorMode::Start) {
        let (bytes, value) = row.map_err(backend)?;
        let key = Key::decode(Store::History, &bytes)?;
        if let Key::Reference { resolution, series } = key {
            let total = legacy_gauge(&value)?;
            previous.insert((resolution, series.metric, series.key.to_owned()), total);
            batch.put_cf(&history, &bytes, key.encode_value(Value::Total(total))?);
        }
    }

    // Windows sort by resolution and time, so each series sees its assignments
    // chronologically. Counter rows and their pending merge operands stay intact.
    for row in snapshot.iterator_cf_opt(&history, scan_options(), IteratorMode::Start) {
        let (bytes, value) = row.map_err(backend)?;
        let key = Key::decode(Store::History, &bytes)?;
        if let Key::Window {
            resolution, series, ..
        } = key
        {
            if series.kind != MetricKind::Gauge {
                continue;
            }
            let total = legacy_gauge(&value)?;
            let before = previous
                .entry((resolution, series.metric, series.key.to_owned()))
                .or_insert(0_i128);
            let delta = total.checked_sub(*before).ok_or(Error::Overflow)?;
            *before = total;
            batch.put_cf(&history, &bytes, key.encode_value(Value::Total(delta))?);
        }
    }

    for row in snapshot.iterator_cf_opt(&aggregates, scan_options(), IteratorMode::Start) {
        let (key, _) = row.map_err(backend)?;
        batch.delete_cf(&aggregates, key);
    }
    batch.put(FORMAT_KEY, FORMAT_VERSION);
    let mut options = WriteOptions::default();
    options.set_sync(true);
    database.write_opt(batch, &options).map_err(backend)
}

fn legacy_gauge(bytes: &[u8]) -> Result<i128, Error> {
    let (value, timestamp_and_sequence) = bytes
        .split_first_chunk::<8>()
        .ok_or(Error::CorruptData("truncated legacy gauge"))?;
    if timestamp_and_sequence.len() != 16 {
        return Err(Error::CorruptData("invalid legacy gauge length"));
    }
    Ok(i128::from(i64::from_be_bytes(*value)))
}

fn scan_options() -> ReadOptions {
    let mut options = ReadOptions::default();
    options.set_total_order_seek(true);
    options
}

fn backend(error: rocksdb::Error) -> Error {
    Error::Backend(Box::new(error))
}

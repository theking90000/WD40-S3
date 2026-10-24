//! Native signed gauge additions, counter additions and cache invalidation.

use rocksdb::{MergeOperands, Options};

use crate::{Error, MetricKind};

use super::codec::{CacheEntry, Key, Store, Value};

#[derive(Clone, Copy)]
pub(super) enum Operand {
    Add(i128),
    Invalidate,
}

impl Operand {
    pub(super) fn encode(self) -> Vec<u8> {
        match self {
            Self::Add(delta) => {
                let mut bytes = vec![0];
                bytes.extend_from_slice(&delta.to_be_bytes());
                bytes
            }
            Self::Invalidate => vec![1],
        }
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let (&tag, rest) = bytes.split_first()?;
        match tag {
            0 => Some(Self::Add(i128::from_be_bytes(rest.try_into().ok()?))),
            1 if rest.is_empty() => Some(Self::Invalidate),
            _ => None,
        }
    }
}

pub(super) fn configure(store: Store, options: &mut Options) {
    let name = match store {
        Store::Current => "metrics-current-i128-v2",
        Store::History => "metrics-history-i128-v2",
        Store::Aggregates => "metrics-aggregates-i128-v2",
    };
    options.set_merge_operator(
        name,
        move |key, existing, operands| full(store, key, existing, operands),
        move |key, _, operands| partial(store, key, operands),
    );
}

fn full(
    store: Store,
    key: &[u8],
    existing: Option<&[u8]>,
    operands: &MergeOperands,
) -> Option<Vec<u8>> {
    let key = Key::decode(store, key).ok()?;
    let mut value = match existing {
        Some(bytes) => key.decode_value(bytes).ok()?,
        None => match key {
            Key::AggregateCurrent(_) => Value::Cache(None),
            Key::AggregateHistory {
                entry: CacheEntry::Valid,
                ..
            } => Value::Valid(false),
            _ => Value::Total(0),
        },
    };
    for bytes in operands {
        let operand = Operand::decode(bytes)?;
        value = match (value, operand) {
            (Value::Cache(Some(total)), Operand::Add(delta)) => match total.checked_add(delta) {
                Some(total) => Value::Cache(Some(total)),
                None => Value::Overflow,
            },
            (Value::Cache(None), Operand::Add(_)) => Value::Cache(None),
            (Value::Cache(_) | Value::Overflow, Operand::Invalidate)
                if matches!(key, Key::AggregateCurrent(_)) =>
            {
                Value::Cache(None)
            }
            (Value::Valid(_), Operand::Invalidate) => Value::Valid(false),
            (Value::Overflow, Operand::Add(_)) => Value::Overflow,
            (Value::Total(total), Operand::Add(delta)) => match total.checked_add(delta) {
                Some(total) => Value::Total(total),
                None => Value::Overflow,
            },
            _ => return None,
        };
    }
    key.encode_value(value).ok()
}

fn partial(store: Store, key: &[u8], operands: &MergeOperands) -> Option<Vec<u8>> {
    let key = Key::decode(store, key).ok()?;
    if matches!(
        key,
        Key::AggregateCurrent(_)
            | Key::AggregateHistory {
                entry: CacheEntry::Valid,
                ..
            }
    ) {
        let mut invalidated = false;
        for bytes in operands {
            match Operand::decode(bytes)? {
                Operand::Invalidate => invalidated = true,
                Operand::Add(_) if matches!(key, Key::AggregateCurrent(_)) => {}
                _ => return None,
            }
        }
        if invalidated {
            return Some(Operand::Invalidate.encode());
        }
        return None;
    }
    // Signed additions cannot be regrouped without changing checked-overflow
    // behavior. Keep them ordered for a full merge against their base value.
    let counter = match key {
        Key::Window { series, .. }
        | Key::AggregateHistory {
            series,
            entry: CacheEntry::Window(_),
            ..
        } => series.kind == MetricKind::Counter,
        _ => false,
    };
    if !counter {
        return None;
    }
    let mut total = 0_i128;
    for bytes in operands {
        match Operand::decode(bytes)? {
            Operand::Add(delta) if delta >= 0 => total = total.checked_add(delta)?,
            _ => return None,
        }
    }
    Some(Operand::Add(total).encode())
}

pub(super) fn poisoned() -> Error {
    Error::Backend(Box::new(std::io::Error::other(
        "metrics coordination lock is poisoned",
    )))
}

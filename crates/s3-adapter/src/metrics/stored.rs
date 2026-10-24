//! Pure conversion of visible object transitions to signed storage gauge deltas.

use metrics_registry::{Metric, MetricKind, MetricRecord, Record};
use object_registry::{DeleteResult, Metadata, RecordMetadata};
use s3s::{S3Error, S3Result};

use crate::metadata;

use super::MetricsContext;

pub(crate) struct Contribution {
    size: i64,
    credential: Option<String>,
}

impl Contribution {
    pub(crate) fn from_metadata(metadata: &Metadata) -> S3Result<Self> {
        Ok(Self {
            size: metadata::content_length(metadata::size(metadata)?)?,
            credential: metadata::stored_credential(metadata).map(str::to_owned),
        })
    }
}

pub(crate) fn store_records(
    key: &str,
    previous: Option<&RecordMetadata>,
    next: &Contribution,
) -> S3Result<Vec<Record>> {
    let previous = contribution(previous)?;
    transition_records(key, previous.as_ref(), Some(next))
}

pub(crate) fn delete_records(key: &str, result: &DeleteResult) -> S3Result<Vec<Record>> {
    if result.previous.as_ref() == result.current.as_ref() {
        return Ok(Vec::new());
    }
    let previous = contribution(result.previous.as_ref())?;
    let current = contribution(result.current.as_ref())?;
    transition_records(key, previous.as_ref(), current.as_ref())
}

fn contribution(record: Option<&RecordMetadata>) -> S3Result<Option<Contribution>> {
    record
        .map(|record| Contribution::from_metadata(&record.metadata))
        .transpose()
}

fn transition_records(
    object_key: &str,
    previous: Option<&Contribution>,
    current: Option<&Contribution>,
) -> S3Result<Vec<Record>> {
    let (bucket, key) = object_key
        .split_once('/')
        .filter(|(bucket, key)| !bucket.is_empty() && !key.is_empty())
        .ok_or_else(|| s3s::s3_error!(InternalError, "invalid stored object key"))?;
    let bytes = current
        .map_or(0, |value| value.size)
        .checked_sub(previous.map_or(0, |value| value.size))
        .ok_or_else(|| {
            S3Error::internal_error(std::io::Error::other("stored byte delta overflow"))
        })?;
    let objects = match (previous.is_some(), current.is_some()) {
        (false, true) => 1,
        (true, false) => -1,
        _ => 0,
    };
    let mut records = Vec::new();
    append(
        &mut records,
        || MetricsContext::stored_bucket(bucket, key),
        bytes,
        objects,
    );

    // Missing legacy attribution contributes only to the bucket namespace.
    let previous_credential = previous.and_then(|value| value.credential.as_deref());
    let current_credential = current.and_then(|value| value.credential.as_deref());
    if previous_credential == current_credential {
        if let Some(credential) = current_credential {
            append(
                &mut records,
                || MetricsContext::stored_credential(credential, bucket, key),
                bytes,
                objects,
            );
        }
    } else {
        if let Some(previous) = previous {
            if let Some(credential) = &previous.credential {
                let bytes = previous.size.checked_neg().ok_or_else(|| {
                    S3Error::internal_error(std::io::Error::other("stored byte delta overflow"))
                })?;
                append(
                    &mut records,
                    || MetricsContext::stored_credential(credential, bucket, key),
                    bytes,
                    -1,
                );
            }
        }
        if let Some(current) = current {
            if let Some(credential) = &current.credential {
                append(
                    &mut records,
                    || MetricsContext::stored_credential(credential, bucket, key),
                    current.size,
                    1,
                );
            }
        }
    }
    Ok(records)
}

fn append(
    records: &mut Vec<Record>,
    context: impl FnOnce() -> MetricsContext,
    bytes: i64,
    objects: i64,
) {
    let metrics: Vec<_> = [
        MetricRecord {
            metric: Metric::StoredBytes,
            kind: MetricKind::Gauge,
            value: bytes,
        },
        MetricRecord {
            metric: Metric::StoredObjects,
            kind: MetricKind::Gauge,
            value: objects,
        },
    ]
    .into_iter()
    .filter(|metric| metric.value != 0)
    .collect();
    if !metrics.is_empty() {
        records.extend(context().records(metrics));
    }
}

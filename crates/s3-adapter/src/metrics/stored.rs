//! Owned final mutations and signed changes to visible logical storage.

use metrics_registry::traits::MetricsRegistry as _;
use metrics_registry::{Metric, MetricKind, MetricRecord, MetricsRegistry, Record};
use object_registry::{
    BlobId, DeleteResult, Metadata, ObjectRegistry, RecordMetadata, StoreResult, VersionId,
    traits::ObjectRegistry as _,
};
use s3s::{S3Error, S3Result};
use tokio::sync::oneshot;

use crate::{error, metadata};

use super::MetricsContext;

/// Once queued, a mutation belongs to the task even if its reply is abandoned.
pub(super) enum Mutation {
    Store {
        objects: ObjectRegistry,
        key: String,
        blob_id: BlobId,
        metadata: Metadata,
        reply: oneshot::Sender<S3Result<StoreResult>>,
    },
    Delete {
        objects: ObjectRegistry,
        key: String,
        version_id: Option<VersionId>,
        reply: oneshot::Sender<S3Result<DeleteResult>>,
    },
}

impl Mutation {
    pub(super) async fn apply(self, registry: &MetricsRegistry) {
        match self {
            Self::Store {
                objects,
                key,
                blob_id,
                metadata,
                reply,
            } => {
                // Validate new accounting data before committing the object.
                let next = match Contribution::from_metadata(&metadata) {
                    Ok(next) => next,
                    Err(error) => {
                        let _ = reply.send(Err(error));
                        return;
                    }
                };
                let result = objects
                    .store(&key, blob_id, metadata)
                    .await
                    .map_err(error::object_registry_error);
                if let Ok(result) = &result {
                    publish(
                        registry,
                        store_records(&key, result.previous.as_ref(), &next),
                    )
                    .await;
                }
                // Registry failures may follow a commit without returning its
                // snapshots. No safe delta can be inferred in that case.
                let _ = reply.send(result);
            }
            Self::Delete {
                objects,
                key,
                version_id,
                reply,
            } => {
                let result = objects
                    .delete(&key, version_id)
                    .await
                    .map_err(error::object_registry_error);
                if let Ok(result) = &result {
                    publish(registry, delete_records(&key, result)).await;
                }
                let _ = reply.send(result);
            }
        }
    }
}

struct Contribution {
    size: i64,
    credential: Option<String>,
}

impl Contribution {
    fn from_metadata(metadata: &Metadata) -> S3Result<Self> {
        Ok(Self {
            size: metadata::content_length(metadata::size(metadata)?)?,
            credential: metadata::stored_credential(metadata).map(str::to_owned),
        })
    }
}

fn store_records(
    key: &str,
    previous: Option<&RecordMetadata>,
    next: &Contribution,
) -> S3Result<Vec<Record>> {
    let mut records = Vec::new();
    if let Some(previous) = previous {
        let before = Contribution::from_metadata(&previous.metadata)?;
        append(&mut records, key, &before, true)?;
    }
    append(&mut records, key, next, false)?;
    Ok(records)
}

fn delete_records(key: &str, result: &DeleteResult) -> S3Result<Vec<Record>> {
    if result.previous.as_ref() == result.current.as_ref() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    if let Some(previous) = &result.previous {
        let before = Contribution::from_metadata(&previous.metadata)?;
        append(&mut records, key, &before, true)?;
    }
    if let Some(current) = &result.current {
        let after = Contribution::from_metadata(&current.metadata)?;
        append(&mut records, key, &after, false)?;
    }
    Ok(records)
}

fn append(
    records: &mut Vec<Record>,
    object_key: &str,
    contribution: &Contribution,
    remove: bool,
) -> S3Result<()> {
    if contribution.size == 0 {
        return Ok(());
    }
    let (bucket, key) = object_key
        .split_once('/')
        .filter(|(bucket, key)| !bucket.is_empty() && !key.is_empty())
        .ok_or_else(|| s3s::s3_error!(InternalError, "invalid stored object key"))?;
    let value = if remove {
        contribution.size.checked_neg().ok_or_else(|| {
            S3Error::internal_error(std::io::Error::other("stored byte delta overflow"))
        })?
    } else {
        contribution.size
    };
    records.extend(
        MetricsContext::stored_object(contribution.credential.as_deref(), bucket, key).records(
            vec![MetricRecord {
                metric: Metric::StoredBytes,
                kind: MetricKind::Gauge,
                value,
            }],
        ),
    );
    Ok(())
}

async fn publish(registry: &MetricsRegistry, records: S3Result<Vec<Record>>) {
    let records = match records {
        Ok(records) => records,
        Err(error) => {
            eprintln!("S3 stored-byte accounting failed: {error}");
            return;
        }
    };
    if !records.is_empty() {
        if let Err(error) = registry.record(records).await {
            // Replaying an ambiguous delta batch could double-count it.
            eprintln!("S3 stored-byte recording failed: {error}");
        }
    }
}

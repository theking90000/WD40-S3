use axum::{Extension, Json, extract::State, http::StatusCode};
use metrics_registry::{
    Metric, MetricKind, MetricQuery, MetricResult, MetricsRegistry, Query, Selection,
};
use portal_db::{
    Bucket, DatabaseConnection, bucket,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder},
};
use serde::Serialize;

use super::{BucketResponse, CurrentOrganization, Error, metric_prefix};

#[derive(Serialize)]
pub(super) struct BucketListResponse {
    #[serde(flatten)]
    bucket: BucketResponse,
    stored_bytes: u64,
    stored_objects: u64,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    Extension(metrics): Extension<MetricsRegistry>,
) -> Result<Json<Vec<BucketListResponse>>, Error> {
    let buckets = bucket::Entity::find()
        .filter(bucket::Column::OrganizationId.eq(organization.id))
        .order_by_asc(bucket::Column::Name)
        .all(&db)
        .await?;

    read_usage(&metrics, buckets)
        .await
        .map(Json)
        .map_err(|error| {
            eprintln!("Bucket metrics query failed: {error}");
            Error::new(StatusCode::SERVICE_UNAVAILABLE, "metrics_unavailable")
        })
}

async fn read_usage(
    registry: &impl metrics_registry::traits::MetricsRegistry,
    buckets: Vec<Bucket>,
) -> Result<Vec<BucketListResponse>, metrics_registry::Error> {
    if buckets.is_empty() {
        return Ok(Vec::new());
    }
    let queries = buckets
        .iter()
        .map(|bucket| Query {
            prefixes: vec![metric_prefix(&bucket.name)],
            metrics: [Metric::StoredBytes, Metric::StoredObjects]
                .map(|metric| MetricQuery {
                    metric,
                    kind: MetricKind::Gauge,
                    selection: Selection::Current,
                })
                .to_vec(),
        })
        .collect();
    let results = registry.query(queries).await?;
    if results.len() != buckets.len() {
        return Err(invalid_result());
    }
    buckets
        .into_iter()
        .zip(results)
        .map(|(bucket, result)| {
            let [MetricResult::Current(bytes), MetricResult::Current(objects)] =
                result.metrics.as_slice()
            else {
                return Err(invalid_result());
            };
            Ok(BucketListResponse {
                bucket: BucketResponse::from(bucket),
                stored_bytes: u64::try_from(*bytes).map_err(|_| invalid_result())?,
                stored_objects: u64::try_from(*objects).map_err(|_| invalid_result())?,
            })
        })
        .collect()
}

fn invalid_result() -> metrics_registry::Error {
    metrics_registry::Error::InvalidQuery("invalid bucket metrics result".into())
}

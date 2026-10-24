pub(super) mod uploads;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{Extension, Json, extract::State, http::StatusCode};
use metrics_registry::{
    Metric, MetricKind, MetricQuery, MetricResult, MetricsRegistry, Query, Selection,
};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use portal_db::{
    DatabaseConnection, bucket,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect},
};
use serde::Serialize;

use super::{Error, current::CurrentOrganization};

const RESOLUTION: u64 = 30 * 60;
const DAY: u64 = 24 * 60 * 60;
// Match s3-adapter's bucket segment encoding and preserve the trailing delimiter.
const SEGMENT: &AsciiSet = &CONTROLS.add(b'/').add(b'%');

#[derive(Debug, Serialize)]
pub(super) struct DashboardResponse {
    generated_at: u64,
    period: Period,
    stored_bytes: u64,
    stored_objects: u64,
    requests: u64,
    uploaded_bytes: u64,
    downloaded_bytes: u64,
    transferred_bytes: u64,
    buckets: Vec<BucketUsage>,
}

#[derive(Debug, Serialize)]
struct Period {
    from: u64,
    to: u64,
    resolution_seconds: u64,
}

#[derive(Debug, Serialize)]
struct BucketUsage {
    name: String,
    stored_bytes: u64,
    stored_objects: u64,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    Extension(metrics): Extension<MetricsRegistry>,
) -> Result<Json<DashboardResponse>, Error> {
    let names = bucket_names(&db, organization).await?;
    read_dashboard(&metrics, names, SystemTime::now())
        .await
        .map(Json)
        .map_err(|error| {
            eprintln!("Dashboard metrics query failed: {error}");
            Error::new(StatusCode::SERVICE_UNAVAILABLE, "metrics_unavailable")
        })
}

async fn bucket_names(
    db: &DatabaseConnection,
    organization: CurrentOrganization,
) -> Result<Vec<String>, Error> {
    Ok(bucket::Entity::find()
        .select_only()
        .column(bucket::Column::Name)
        .filter(bucket::Column::OrganizationId.eq(organization.id))
        .order_by_asc(bucket::Column::Name)
        .into_tuple()
        .all(db)
        .await?)
}

fn bucket_prefixes(names: &[String]) -> Vec<String> {
    names
        .iter()
        .map(|name| format!("/by-buckets/{}/", utf8_percent_encode(name, SEGMENT)))
        .collect()
}

async fn read_dashboard(
    registry: &impl metrics_registry::traits::MetricsRegistry,
    names: Vec<String>,
    now: SystemTime,
) -> Result<DashboardResponse, metrics_registry::Error> {
    let generated_at = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid_result())?
        .as_secs();
    // Keep the last 48 closed windows and include the current window's increments.
    let window_start = generated_at / RESOLUTION * RESOLUTION;
    let from = window_start.checked_sub(DAY).ok_or_else(invalid_result)?;
    let to = generated_at;
    let history = Selection::History {
        resolution: Duration::from_secs(RESOLUTION),
        range: UNIX_EPOCH + Duration::from_secs(from)..now,
    };
    let prefixes = bucket_prefixes(&names);
    let mut queries = vec![Query {
        // An empty list selects no keys, including for an organization with no buckets.
        prefixes: prefixes.clone(),
        metrics: [
            Metric::RequestCount,
            Metric::UploadedBytes,
            Metric::DownloadedBytes,
        ]
        .map(|metric| MetricQuery {
            metric,
            kind: MetricKind::Counter,
            selection: history.clone(),
        })
        .to_vec(),
    }];
    queries.extend(prefixes.into_iter().map(|prefix| {
        Query {
            prefixes: vec![prefix],
            metrics: [Metric::StoredBytes, Metric::StoredObjects]
                .map(|metric| MetricQuery {
                    metric,
                    kind: MetricKind::Gauge,
                    selection: Selection::Current,
                })
                .to_vec(),
        }
    }));
    let results = registry.query(queries).await?;
    if results.len() != names.len() + 1 {
        return Err(invalid_result());
    }
    let mut results = results.into_iter();
    let counters = results.next().ok_or_else(invalid_result)?;
    let [requests, uploaded, downloaded] = counters.metrics.as_slice() else {
        return Err(invalid_result());
    };
    let requests = history_total(requests)?;
    let uploaded_bytes = history_total(uploaded)?;
    let downloaded_bytes = history_total(downloaded)?;
    let transferred_bytes = add(uploaded_bytes, downloaded_bytes)?;
    let mut stored_bytes = 0;
    let mut stored_objects = 0;
    let mut buckets = Vec::with_capacity(names.len());
    for (name, result) in names.into_iter().zip(results) {
        let [bytes, objects] = result.metrics.as_slice() else {
            return Err(invalid_result());
        };
        let bytes = current(bytes)?;
        let objects = current(objects)?;
        stored_bytes = add(stored_bytes, bytes)?;
        stored_objects = add(stored_objects, objects)?;
        buckets.push(BucketUsage {
            name,
            stored_bytes: bytes,
            stored_objects: objects,
        });
    }
    Ok(DashboardResponse {
        generated_at,
        period: Period {
            from,
            to,
            resolution_seconds: RESOLUTION,
        },
        stored_bytes,
        stored_objects,
        requests,
        uploaded_bytes,
        downloaded_bytes,
        transferred_bytes,
        buckets,
    })
}

fn current(result: &MetricResult) -> Result<u64, metrics_registry::Error> {
    match result {
        MetricResult::Current(value) => u64::try_from(*value).map_err(|_| invalid_result()),
        _ => Err(invalid_result()),
    }
}

fn history_total(result: &MetricResult) -> Result<u64, metrics_registry::Error> {
    match result {
        MetricResult::History(points) => points.iter().try_fold(0, |total, point| {
            add(
                total,
                u64::try_from(point.value).map_err(|_| invalid_result())?,
            )
        }),
        _ => Err(invalid_result()),
    }
}

fn add(left: u64, right: u64) -> Result<u64, metrics_registry::Error> {
    left.checked_add(right)
        .ok_or(metrics_registry::Error::Overflow)
}

fn invalid_result() -> metrics_registry::Error {
    metrics_registry::Error::InvalidQuery("invalid dashboard metrics result".into())
}

#[cfg(test)]
mod tests {
    use std::{future::Future, sync::Mutex};

    use metrics_registry::{Point, QueryResult, Record};

    use super::*;

    struct Registry {
        calls: Mutex<Vec<Vec<Query<u16>>>>,
        result: Mutex<Option<Result<Vec<QueryResult>, metrics_registry::Error>>>,
    }

    impl metrics_registry::traits::MetricsRegistry for Registry {
        fn record<M: Into<u16>>(
            &self,
            _: Vec<Record<M>>,
        ) -> impl Future<Output = Result<(), metrics_registry::Error>> + Send {
            async { panic!("dashboard must never write metrics") }
        }

        fn query<M: Into<u16>>(
            &self,
            queries: Vec<Query<M>>,
        ) -> impl Future<Output = Result<Vec<QueryResult>, metrics_registry::Error>> + Send
        {
            self.calls.lock().unwrap().push(
                queries
                    .into_iter()
                    .map(|query| Query {
                        prefixes: query.prefixes,
                        metrics: query
                            .metrics
                            .into_iter()
                            .map(|metric| MetricQuery {
                                metric: metric.metric.into(),
                                kind: metric.kind,
                                selection: metric.selection,
                            })
                            .collect(),
                    })
                    .collect(),
            );
            std::future::ready(self.result.lock().unwrap().take().expect("one batch only"))
        }
    }

    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(2_000_000_123) + Duration::from_millis(500)
    }

    fn counters(value: i64) -> QueryResult {
        let to = now().duration_since(UNIX_EPOCH).unwrap().as_secs() / RESOLUTION * RESOLUTION;
        QueryResult {
            metrics: [value, value * 2, value * 3]
                .map(|value| {
                    MetricResult::History(
                        (0..=48)
                            .map(|index| Point {
                                start: UNIX_EPOCH
                                    + Duration::from_secs(to - DAY + index * RESOLUTION),
                                value,
                                complete: index < 48,
                            })
                            .collect(),
                    )
                })
                .to_vec(),
        }
    }

    fn registry(results: Vec<QueryResult>) -> Registry {
        Registry {
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(Some(Ok(results))),
        }
    }

    #[tokio::test]
    async fn one_batch_scopes_buckets_and_includes_current_window() {
        let registry = registry(vec![
            counters(1),
            QueryResult {
                metrics: vec![MetricResult::Current(100), MetricResult::Current(2)],
            },
            QueryResult {
                metrics: vec![MetricResult::Current(300), MetricResult::Current(5)],
            },
        ]);
        let dashboard = read_dashboard(
            &registry,
            vec!["photos".into(), "photos-archive/%".into()],
            now(),
        )
        .await
        .unwrap();
        assert_eq!((dashboard.stored_bytes, dashboard.stored_objects), (400, 7));
        assert_eq!(
            (
                dashboard.requests,
                dashboard.uploaded_bytes,
                dashboard.downloaded_bytes
            ),
            (49, 98, 147)
        );
        assert_eq!(dashboard.transferred_bytes, 245);
        assert_eq!(dashboard.buckets.len(), 2);
        assert_eq!(dashboard.buckets[1].stored_bytes, 300);
        assert_eq!(dashboard.buckets[1].stored_objects, 5);
        assert_eq!(dashboard.period.from % RESOLUTION, 0);
        assert_eq!(dashboard.period.to, dashboard.generated_at);
        assert!((DAY..DAY + RESOLUTION).contains(&(dashboard.period.to - dashboard.period.from)));

        let calls = registry.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let queries = &calls[0];
        assert_eq!(queries.len(), 3);
        assert_eq!(
            queries[0].prefixes,
            ["/by-buckets/photos/", "/by-buckets/photos-archive%2F%25/"]
        );
        for (metric, expected) in queries[0].metrics.iter().zip([
            Metric::RequestCount,
            Metric::UploadedBytes,
            Metric::DownloadedBytes,
        ]) {
            assert_eq!(metric.metric, u16::from(expected));
            assert_eq!(metric.kind, MetricKind::Counter);
            assert_eq!(
                metric.selection,
                Selection::History {
                    resolution: Duration::from_secs(RESOLUTION),
                    range: UNIX_EPOCH + Duration::from_secs(dashboard.period.from)..now(),
                }
            );
        }
        for (index, query) in queries[1..].iter().enumerate() {
            assert_eq!(query.prefixes, vec![queries[0].prefixes[index].clone()]);
            assert_eq!(
                query
                    .metrics
                    .iter()
                    .map(|metric| metric.metric)
                    .collect::<Vec<_>>(),
                vec![
                    u16::from(Metric::StoredBytes),
                    u16::from(Metric::StoredObjects)
                ]
            );
            assert!(
                query
                    .metrics
                    .iter()
                    .all(|metric| metric.kind == MetricKind::Gauge
                        && metric.selection == Selection::Current)
            );
        }
    }

    #[tokio::test]
    async fn no_buckets_selects_nothing_and_unwritten_gauges_remain_zero() {
        let empty = registry(vec![counters(0)]);
        let dashboard = read_dashboard(&empty, Vec::new(), now()).await.unwrap();
        assert_eq!(
            (
                dashboard.stored_bytes,
                dashboard.stored_objects,
                dashboard.requests,
                dashboard.transferred_bytes
            ),
            (0, 0, 0, 0)
        );
        assert!(dashboard.buckets.is_empty());
        assert!(empty.calls.lock().unwrap()[0][0].prefixes.is_empty());

        let unwritten = registry(vec![
            counters(0),
            QueryResult {
                metrics: vec![MetricResult::Current(0), MetricResult::Current(0)],
            },
        ]);
        let dashboard = read_dashboard(&unwritten, vec!["photos".into()], now())
            .await
            .unwrap();
        assert_eq!((dashboard.stored_bytes, dashboard.stored_objects), (0, 0));
        assert_eq!(dashboard.buckets[0].stored_objects, 0);
    }

    #[tokio::test]
    async fn failures_and_invalid_values_are_not_reported_as_zero() {
        let unavailable = Registry {
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(Some(Err(metrics_registry::Error::ResolutionUnavailable(
                Duration::from_secs(RESOLUTION),
            )))),
        };
        assert!(matches!(
            read_dashboard(&unavailable, Vec::new(), now()).await,
            Err(metrics_registry::Error::ResolutionUnavailable(_))
        ));
        let negative = registry(vec![
            counters(0),
            QueryResult {
                metrics: vec![MetricResult::Current(-1), MetricResult::Current(0)],
            },
        ]);
        assert!(
            read_dashboard(&negative, vec!["photos".into()], now())
                .await
                .is_err()
        );
        let overflow = registry(vec![counters(i64::MAX / 3)]);
        assert!(matches!(
            read_dashboard(&overflow, Vec::new(), now()).await,
            Err(metrics_registry::Error::Overflow)
        ));
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn bucket_selection_excludes_other_organizations() {
        use portal_db::{
            BucketAccess, Database, Migrator, MigratorTrait, OrganizationMemberAccess,
            organization,
            sea_orm::{ActiveModelTrait, Set},
        };

        let db = Database::connect("sqlite::memory:").await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let mut ids = Vec::new();
        for (name, buckets) in [
            ("First", vec!["photos", "archive"]),
            ("Second", vec!["photos-other"]),
        ] {
            let organization = organization::ActiveModel {
                name: Set(name.into()),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
            ids.push(organization.id);
            for bucket in buckets {
                bucket::ActiveModel {
                    name: Set(bucket.into()),
                    organization_id: Set(organization.id),
                    access: Set(BucketAccess::Private),
                    ..Default::default()
                }
                .insert(&db)
                .await
                .unwrap();
            }
        }
        let names = bucket_names(
            &db,
            CurrentOrganization {
                id: ids[0],
                access: OrganizationMemberAccess::Member,
            },
        )
        .await
        .unwrap_or_else(|_| panic!("bucket lookup failed"));
        assert_eq!(names, ["archive", "photos"]);
        db.close().await.unwrap();
    }
}

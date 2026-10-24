use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Extension, Json,
    extract::{Query as ParametersQuery, State, rejection::QueryRejection},
    http::StatusCode,
};
use metrics_registry::{
    Metric, MetricKind, MetricQuery, MetricResult, MetricsRegistry, Query, Selection,
};
use portal_db::DatabaseConnection;
use serde::{Deserialize, Serialize};

use super::super::{Error, current::CurrentOrganization};
use super::{DAY, Period, add, bucket_names, bucket_prefixes, invalid_result};

#[derive(Default, Deserialize)]
enum UploadPeriod {
    #[default]
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
}

impl UploadPeriod {
    fn days(&self) -> u64 {
        match self {
            Self::Week => 7,
            Self::Month => 30,
        }
    }
}

#[derive(Deserialize)]
pub(in super::super) struct Parameters {
    #[serde(default)]
    period: UploadPeriod,
}

#[derive(Serialize)]
pub(in super::super) struct UploadsResponse {
    generated_at: u64,
    period: Period,
    total_uploaded_bytes: u64,
    points: Vec<UploadPoint>,
}

#[derive(Serialize)]
struct UploadPoint {
    start: u64,
    uploaded_bytes: u64,
    complete: bool,
}

pub(in super::super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    Extension(metrics): Extension<MetricsRegistry>,
    parameters: Result<ParametersQuery<Parameters>, QueryRejection>,
) -> Result<Json<UploadsResponse>, Error> {
    let ParametersQuery(parameters) =
        parameters.map_err(|_| Error::new(StatusCode::BAD_REQUEST, "invalid_period"))?;
    let names = bucket_names(&db, organization).await?;
    read_uploads(&metrics, names, parameters.period, SystemTime::now())
        .await
        .map(Json)
        .map_err(|error| {
            eprintln!("Dashboard uploads query failed: {error}");
            Error::new(StatusCode::SERVICE_UNAVAILABLE, "metrics_unavailable")
        })
}

async fn read_uploads(
    registry: &impl metrics_registry::traits::MetricsRegistry,
    names: Vec<String>,
    period: UploadPeriod,
    now: SystemTime,
) -> Result<UploadsResponse, metrics_registry::Error> {
    let generated_at = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid_result())?
        .as_secs();
    let today = generated_at
        .checked_rem(DAY)
        .and_then(|remainder| generated_at.checked_sub(remainder))
        .ok_or_else(invalid_result)?;
    let lookback = period
        .days()
        .checked_sub(1)
        .and_then(|days| days.checked_mul(DAY))
        .ok_or_else(invalid_result)?;
    let from = today.checked_sub(lookback).ok_or_else(invalid_result)?;
    let start = UNIX_EPOCH
        .checked_add(Duration::from_secs(from))
        .ok_or_else(invalid_result)?;
    let results = registry
        .query(vec![Query {
            prefixes: bucket_prefixes(&names),
            metrics: vec![MetricQuery {
                metric: Metric::UploadedBytes,
                kind: MetricKind::Counter,
                selection: Selection::History {
                    resolution: Duration::from_secs(DAY),
                    range: start..now,
                },
            }],
        }])
        .await?;
    let [result] = results.as_slice() else {
        return Err(invalid_result());
    };
    let [MetricResult::History(history)] = result.metrics.as_slice() else {
        return Err(invalid_result());
    };
    let mut points = Vec::new();
    let mut total_uploaded_bytes = 0;
    let mut expected_start = from;
    for point in history {
        let expected_time = UNIX_EPOCH
            .checked_add(Duration::from_secs(expected_start))
            .ok_or_else(invalid_result)?;
        if expected_start > today || point.start != expected_time {
            return Err(invalid_result());
        }
        let uploaded_bytes = u64::try_from(point.value).map_err(|_| invalid_result())?;
        total_uploaded_bytes = add(total_uploaded_bytes, uploaded_bytes)?;
        points.push(UploadPoint {
            start: expected_start,
            uploaded_bytes,
            complete: point.complete,
        });
        expected_start = add(expected_start, DAY)?;
    }
    // At exactly midnight, the end-exclusive range can omit today's empty window.
    if expected_start == today
        && now
            == UNIX_EPOCH
                .checked_add(Duration::from_secs(today))
                .ok_or_else(invalid_result)?
    {
        points.push(UploadPoint {
            start: today,
            uploaded_bytes: 0,
            complete: false,
        });
        expected_start = add(expected_start, DAY)?;
    }
    if expected_start != add(today, DAY)? {
        return Err(invalid_result());
    }
    Ok(UploadsResponse {
        generated_at,
        period: Period {
            from,
            to: generated_at,
            resolution_seconds: DAY,
        },
        total_uploaded_bytes,
        points,
    })
}

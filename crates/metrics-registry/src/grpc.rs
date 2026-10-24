//! Protocol v1 and a service delegating each complete batch to the injected registry.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use prost::Message;
use tonic::{Code, Status};

#[cfg(feature = "grpc-client")]
use crate::Point;
use crate::{Error, MetricKind, MetricResult, Query, QueryResult, Record, Selection};
#[cfg(feature = "grpc-server")]
use crate::{MetricQuery, MetricRecord};

pub mod proto {
    tonic::include_proto!("wd40.metrics_registry.v1");
}

/// Maximum encoded or decoded message size for both transports.
pub const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

fn encode_duration(value: Duration) -> proto::Duration {
    proto::Duration {
        seconds: value.as_secs(),
        nanos: value.subsec_nanos(),
    }
}

fn decode_duration(value: proto::Duration) -> Result<Duration, Status> {
    if value.nanos >= 1_000_000_000 {
        return Err(Status::invalid_argument(
            "duration nanoseconds are out of range",
        ));
    }
    Ok(Duration::new(value.seconds, value.nanos))
}

fn encode_timestamp(value: SystemTime) -> proto::Timestamp {
    let (before_epoch, duration) = match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => (false, duration),
        Err(error) => (true, error.duration()),
    };
    proto::Timestamp {
        before_epoch,
        seconds: duration.as_secs(),
        nanos: duration.subsec_nanos(),
    }
}

fn decode_timestamp(value: proto::Timestamp) -> Result<SystemTime, Status> {
    let duration = decode_duration(proto::Duration {
        seconds: value.seconds,
        nanos: value.nanos,
    })?;
    let time = if value.before_epoch {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    };
    time.ok_or_else(|| Status::invalid_argument("timestamp cannot be represented"))
}

#[cfg(feature = "grpc-client")]
fn encode_kind(kind: MetricKind) -> i32 {
    (match kind {
        MetricKind::Gauge => proto::MetricKind::Gauge,
        MetricKind::Counter => proto::MetricKind::Counter,
    }) as i32
}

#[cfg(feature = "grpc-server")]
fn decode_kind(kind: i32) -> Result<MetricKind, Status> {
    match proto::MetricKind::try_from(kind) {
        Ok(proto::MetricKind::Gauge) => Ok(MetricKind::Gauge),
        Ok(proto::MetricKind::Counter) => Ok(MetricKind::Counter),
        _ => Err(Status::invalid_argument("missing or unknown metric kind")),
    }
}

#[cfg(feature = "grpc-server")]
fn decode_metric(metric: Option<u32>) -> Result<u16, Status> {
    let metric = metric.ok_or_else(|| Status::invalid_argument("missing metric identifier"))?;
    u16::try_from(metric).map_err(|_| Status::invalid_argument("metric identifier exceeds u16"))
}

#[cfg(feature = "grpc-client")]
pub(crate) fn encode_records<Metric: Into<u16>>(
    records: Vec<Record<Metric>>,
) -> proto::RecordRequest {
    proto::RecordRequest {
        records: records
            .into_iter()
            .map(|record| proto::Record {
                key: record.key,
                aggregates: record.aggregates,
                metrics: record
                    .metrics
                    .into_iter()
                    .map(|metric| {
                        let id: u16 = metric.metric.into();
                        proto::MetricRecord {
                            metric: Some(u32::from(id)),
                            kind: encode_kind(metric.kind),
                            value: Some(metric.value),
                        }
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[cfg(feature = "grpc-server")]
fn decode_records(request: proto::RecordRequest) -> Result<Vec<Record<u16>>, Status> {
    request
        .records
        .into_iter()
        .map(|record| {
            let metrics = record
                .metrics
                .into_iter()
                .map(|metric| {
                    Ok(MetricRecord {
                        metric: decode_metric(metric.metric)?,
                        kind: decode_kind(metric.kind)?,
                        value: metric
                            .value
                            .ok_or_else(|| Status::invalid_argument("missing recorded value"))?,
                    })
                })
                .collect::<Result<_, Status>>()?;
            Ok(Record {
                key: record.key,
                aggregates: record.aggregates,
                metrics,
            })
        })
        .collect()
}

#[cfg(feature = "grpc-client")]
pub(crate) fn encode_queries<Metric: Into<u16>>(
    queries: Vec<Query<Metric>>,
) -> proto::QueryRequest {
    use proto::metric_query::Selection as WireSelection;
    proto::QueryRequest {
        queries: queries
            .into_iter()
            .map(|query| proto::Query {
                prefixes: query.prefixes,
                metrics: query
                    .metrics
                    .into_iter()
                    .map(|metric| {
                        let id: u16 = metric.metric.into();
                        let selection = match metric.selection {
                            Selection::Current => WireSelection::Current(proto::Empty {}),
                            Selection::History { resolution, range } => {
                                WireSelection::History(proto::HistorySelection {
                                    resolution: Some(encode_duration(resolution)),
                                    start: Some(encode_timestamp(range.start)),
                                    end: Some(encode_timestamp(range.end)),
                                })
                            }
                        };
                        proto::MetricQuery {
                            metric: Some(u32::from(id)),
                            kind: encode_kind(metric.kind),
                            selection: Some(selection),
                        }
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[cfg(feature = "grpc-server")]
fn decode_queries(request: proto::QueryRequest) -> Result<Vec<Query<u16>>, Status> {
    use proto::metric_query::Selection as WireSelection;
    request
        .queries
        .into_iter()
        .map(|query| {
            let metrics = query
                .metrics
                .into_iter()
                .map(|metric| {
                    let selection = match metric.selection {
                        Some(WireSelection::Current(_)) => Selection::Current,
                        Some(WireSelection::History(history)) => {
                            let resolution =
                                decode_duration(history.resolution.ok_or_else(|| {
                                    Status::invalid_argument("missing history resolution")
                                })?)?;
                            let start = decode_timestamp(history.start.ok_or_else(|| {
                                Status::invalid_argument("missing history start")
                            })?)?;
                            let end =
                                decode_timestamp(history.end.ok_or_else(|| {
                                    Status::invalid_argument("missing history end")
                                })?)?;
                            Selection::History {
                                resolution,
                                range: start..end,
                            }
                        }
                        None => return Err(Status::invalid_argument("missing metric selection")),
                    };
                    Ok(MetricQuery {
                        metric: decode_metric(metric.metric)?,
                        kind: decode_kind(metric.kind)?,
                        selection,
                    })
                })
                .collect::<Result<_, Status>>()?;
            Ok(Query {
                prefixes: query.prefixes,
                metrics,
            })
        })
        .collect()
}

#[cfg(feature = "grpc-server")]
fn encode_results(results: Vec<QueryResult>) -> proto::QueryResponse {
    use proto::metric_result::Value;
    proto::QueryResponse {
        results: results
            .into_iter()
            .map(|result| proto::QueryResult {
                metrics: result
                    .metrics
                    .into_iter()
                    .map(|metric| {
                        let value = match metric {
                            MetricResult::Current(value) => Value::Current(value),
                            MetricResult::History(points) => Value::History(proto::HistoryResult {
                                points: points
                                    .into_iter()
                                    .map(|point| proto::Point {
                                        start: Some(encode_timestamp(point.start)),
                                        value: Some(point.value),
                                        complete: Some(point.complete),
                                    })
                                    .collect(),
                            }),
                        };
                        proto::MetricResult { value: Some(value) }
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[cfg(feature = "grpc-client")]
pub(crate) fn decode_results(
    response: proto::QueryResponse,
    expected: &[Vec<bool>],
) -> Result<Vec<QueryResult>, Status> {
    use proto::metric_result::Value;
    if response.results.len() != expected.len() {
        return Err(Status::data_loss("registry changed query result count"));
    }
    response
        .results
        .into_iter()
        .zip(expected)
        .map(|(result, selections)| {
            if result.metrics.len() != selections.len() {
                return Err(Status::data_loss("registry changed metric result count"));
            }
            let metrics = result
                .metrics
                .into_iter()
                .zip(selections)
                .map(|(metric, current)| match (metric.value, *current) {
                    (Some(Value::Current(value)), true) => Ok(MetricResult::Current(value)),
                    (Some(Value::History(history)), false) => {
                        let mut previous = None;
                        let points = history
                            .points
                            .into_iter()
                            .map(|point| {
                                let start = decode_timestamp(
                                    point
                                        .start
                                        .ok_or_else(|| Status::data_loss("missing point start"))?,
                                )
                                .map_err(|error| Status::data_loss(error.message()))?;
                                if previous.is_some_and(|previous| start <= previous) {
                                    return Err(Status::data_loss(
                                        "history points are not strictly ordered",
                                    ));
                                }
                                previous = Some(start);
                                Ok(Point {
                                    start,
                                    value: point
                                        .value
                                        .ok_or_else(|| Status::data_loss("missing point value"))?,
                                    complete: point.complete.ok_or_else(|| {
                                        Status::data_loss("missing point completeness")
                                    })?,
                                })
                            })
                            .collect::<Result<_, Status>>()?;
                        Ok(MetricResult::History(points))
                    }
                    _ => Err(Status::data_loss("missing or mismatched metric result")),
                })
                .collect::<Result<_, Status>>()?;
            Ok(QueryResult { metrics })
        })
        .collect()
}

#[cfg(feature = "grpc-server")]
pub(crate) fn into_status(error: Error) -> Status {
    use proto::registry_error::Kind;
    let message = error.to_string();
    let (code, kind, reason, resolution) = match error {
        Error::InvalidRecord(reason) => (
            Code::InvalidArgument,
            Kind::InvalidRecord,
            reason.into_owned(),
            None,
        ),
        Error::InvalidQuery(reason) => (
            Code::InvalidArgument,
            Kind::InvalidQuery,
            reason.into_owned(),
            None,
        ),
        Error::ResolutionUnavailable(resolution) => (
            Code::FailedPrecondition,
            Kind::ResolutionUnavailable,
            String::new(),
            Some(encode_duration(resolution)),
        ),
        Error::HistoryExpired => (
            Code::FailedPrecondition,
            Kind::HistoryExpired,
            String::new(),
            None,
        ),
        Error::Overflow => (Code::ResourceExhausted, Kind::Overflow, String::new(), None),
        Error::Rpc(status) => return status,
        #[cfg(feature = "grpc-client")]
        Error::Transport(_) => return Status::unavailable("upstream registry transport failed"),
        _ => return Status::internal("registry storage operation failed"),
    };
    let details = proto::RegistryError {
        kind: kind as i32,
        reason,
        resolution,
    };
    Status::with_details(code, message, details.encode_to_vec().into())
}

#[cfg(feature = "grpc-client")]
pub(crate) fn from_status(status: Status) -> Error {
    use proto::registry_error::Kind;
    let decoded = (|| -> Option<Error> {
        let details = proto::RegistryError::decode(status.details()).ok()?;
        Some(match (status.code(), Kind::try_from(details.kind).ok()?) {
            (Code::InvalidArgument, Kind::InvalidRecord) => {
                Error::InvalidRecord(details.reason.into())
            }
            (Code::InvalidArgument, Kind::InvalidQuery) => {
                Error::InvalidQuery(details.reason.into())
            }
            (Code::FailedPrecondition, Kind::ResolutionUnavailable) => {
                Error::ResolutionUnavailable(decode_duration(details.resolution?).ok()?)
            }
            (Code::FailedPrecondition, Kind::HistoryExpired) => Error::HistoryExpired,
            (Code::ResourceExhausted, Kind::Overflow) => Error::Overflow,
            _ => return None,
        })
    })();
    match decoded {
        Some(error) => error,
        None => Error::Rpc(status),
    }
}

/// gRPC service over a local or remote registry, including relay deployments.
#[cfg(feature = "grpc-server")]
#[derive(Clone, Debug)]
pub struct GrpcService {
    registry: crate::MetricsRegistry,
}

#[cfg(feature = "grpc-server")]
impl GrpcService {
    pub fn new(registry: crate::MetricsRegistry) -> Self {
        Self { registry }
    }

    pub fn into_server(self) -> proto::metrics_registry_server::MetricsRegistryServer<Self> {
        proto::metrics_registry_server::MetricsRegistryServer::new(self)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE)
    }
}

#[cfg(feature = "grpc-server")]
#[tonic::async_trait]
impl proto::metrics_registry_server::MetricsRegistry for GrpcService {
    async fn record(
        &self,
        request: tonic::Request<proto::RecordRequest>,
    ) -> Result<tonic::Response<proto::Empty>, Status> {
        use crate::traits::MetricsRegistry as _;
        let records = decode_records(request.into_inner())?;
        self.registry.record(records).await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::Empty {}))
    }

    async fn query(
        &self,
        request: tonic::Request<proto::QueryRequest>,
    ) -> Result<tonic::Response<proto::QueryResponse>, Status> {
        use crate::traits::MetricsRegistry as _;
        let queries = decode_queries(request.into_inner())?;
        let results = self.registry.query(queries).await.map_err(into_status)?;
        Ok(tonic::Response::new(encode_results(results)))
    }
}

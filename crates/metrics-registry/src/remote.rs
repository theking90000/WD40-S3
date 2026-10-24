//! Connected protocol v2 client. Failed delta batches are not replayed.

use tonic::{Status, transport::Endpoint};

use crate::{
    Error, Query, QueryResult, Record,
    grpc::{self, proto},
    traits::MetricsRegistry,
};

/// Clones share their HTTP/2 channel without an application mutex.
#[derive(Clone, Debug)]
pub struct RemoteRegistry {
    client: proto::metrics_registry_client::MetricsRegistryClient<tonic::transport::Channel>,
}

impl RemoteRegistry {
    /// Connects to the caller's endpoint. Requires a Tokio runtime.
    pub async fn connect(endpoint: Endpoint) -> Result<Self, Error> {
        match endpoint.uri().scheme_str() {
            Some("http") => {}
            Some("https") => {
                #[cfg(not(feature = "grpc-tls"))]
                return Err(Error::Rpc(Status::invalid_argument(
                    "HTTPS requires grpc-tls",
                )));
            }
            _ => {
                return Err(Error::Rpc(Status::invalid_argument(
                    "registry endpoint requires http or https",
                )));
            }
        }
        let channel = endpoint.connect().await.map_err(Error::Transport)?;
        Ok(Self {
            client: proto::metrics_registry_client::MetricsRegistryClient::new(channel)
                .max_decoding_message_size(grpc::MAX_MESSAGE_SIZE)
                .max_encoding_message_size(grpc::MAX_MESSAGE_SIZE),
        })
    }
}

impl MetricsRegistry for RemoteRegistry {
    fn record<Metric: Into<u16>>(
        &self,
        records: Vec<Record<Metric>>,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send {
        let request = grpc::encode_records(records);
        let mut client = self.client.clone();
        async move {
            client.record(request).await.map_err(grpc::from_status)?;
            Ok(())
        }
    }

    fn query<Metric: Into<u16>>(
        &self,
        queries: Vec<Query<Metric>>,
    ) -> impl std::future::Future<Output = Result<Vec<QueryResult>, Error>> + Send {
        let request = grpc::encode_queries(queries);
        let expected: Vec<Vec<bool>> = request
            .queries
            .iter()
            .map(|query| {
                query
                    .metrics
                    .iter()
                    .map(|metric| {
                        matches!(
                            &metric.selection,
                            Some(proto::metric_query::Selection::Current(_))
                        )
                    })
                    .collect()
            })
            .collect();
        let mut client = self.client.clone();
        async move {
            let response = client
                .query(request)
                .await
                .map_err(grpc::from_status)?
                .into_inner();
            grpc::decode_results(response, &expected).map_err(Error::Rpc)
        }
    }
}

//! gRPC registry client. Clones share the transport; failed calls are not replayed.

use futures_util::{StreamExt, stream};
use tonic::{Status, transport::Endpoint};

use crate::{
    Error, FileId, Metadata, StreamId, StreamPart, StreamPartId,
    grpc::{self, proto},
    traits::StreamRegistry,
};

/// A connected registry client. Clones share its HTTP/2 channel.
#[derive(Clone)]
pub struct RemoteRegistry {
    client: proto::stream_registry_client::StreamRegistryClient<tonic::transport::Channel>,
}

impl RemoteRegistry {
    /// Connects using the caller's endpoint, including any TLS configuration.
    /// A Tokio runtime is required. HTTPS requires the `grpc-tls` feature.
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
        // ponytail: no replay or fallback; add operation deduplication before write retries.
        let channel = endpoint.connect().await.map_err(Error::Transport)?;
        Ok(Self {
            client: proto::stream_registry_client::StreamRegistryClient::new(channel)
                .max_decoding_message_size(grpc::MAX_MESSAGE_SIZE)
                .max_encoding_message_size(grpc::MAX_MESSAGE_SIZE),
        })
    }
}

impl StreamRegistry for RemoteRegistry {
    type StreamPartStream = <crate::LocalRegistry as StreamRegistry>::StreamPartStream;

    async fn insert(&self, file_id: FileId, metadata: Metadata) -> Result<StreamPartId, Error> {
        let request = proto::InsertRequest {
            file_id: file_id.to_be_bytes().to_vec(),
            metadata: postcard::to_allocvec(&metadata)?,
        };
        let response = self
            .client
            .clone()
            .insert(request)
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        grpc::decode_id(&response.part_id)
            .map_err(|error| Error::Rpc(Status::data_loss(error.message())))
    }

    async fn store(&self, parts: Vec<StreamPartId>) -> Result<StreamId, Error> {
        let request = proto::StoreRequest {
            part_ids: parts
                .into_iter()
                .map(|id| id.to_be_bytes().to_vec())
                .collect(),
        };
        let response = self
            .client
            .clone()
            .store(request)
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        grpc::decode_id(&response.stream_id)
            .map_err(|error| Error::Rpc(Status::data_loss(error.message())))
    }

    async fn retrieve_parts(&self, ids: &[StreamId]) -> Result<Self::StreamPartStream, Error> {
        let request = proto::RetrievePartsRequest {
            stream_ids: ids.iter().map(|id| id.to_be_bytes().to_vec()).collect(),
        };
        let parts = self
            .client
            .clone()
            .retrieve_parts(request)
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        Ok(Box::pin(
            stream::unfold(Some(parts), |state| async move {
                let mut parts = state?;
                let part = parts.next().await?;
                let item = part.map_err(grpc::from_status).and_then(|part| {
                    let decoded = (|| -> Result<_, Status> {
                        Ok(StreamPart {
                            file_id: grpc::decode_id(&part.file_id)?,
                            metadata: grpc::decode_metadata(&part.metadata)?,
                        })
                    })();
                    decoded.map_err(|error| Error::Rpc(Status::data_loss(error.message())))
                });
                let state = if item.is_ok() { Some(parts) } else { None };
                Some((item, state))
            })
            .fuse(),
        ))
    }
}

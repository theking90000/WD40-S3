//! Connected gRPC client. Failed operations are not replayed.
use crate::{
    BlobId, Error, PartId, StreamId, StreamSlice,
    grpc::{self, proto},
    traits::BlobRegistry,
};
use tonic::{Status, transport::Endpoint};
/// A connected registry client. Clones share its HTTP/2 channel.
#[derive(Clone)]
pub struct RemoteRegistry {
    client: proto::blob_registry_client::BlobRegistryClient<tonic::transport::Channel>,
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
            client: proto::blob_registry_client::BlobRegistryClient::new(channel)
                .max_decoding_message_size(grpc::MAX_MESSAGE_SIZE)
                .max_encoding_message_size(grpc::MAX_MESSAGE_SIZE),
        })
    }
}

impl BlobRegistry for RemoteRegistry {
    type StreamSliceStream = <crate::LocalRegistry as BlobRegistry>::StreamSliceStream;
    async fn create(&self) -> Result<BlobId, Error> {
        let r = self
            .client
            .clone()
            .create(proto::Empty {})
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        grpc::decode_id(&r.blob_id).map_err(|e| Error::Rpc(Status::data_loss(e.message())))
    }
    async fn can_save(&self, blob_id: BlobId, part_id: PartId) -> Result<(), Error> {
        self.client
            .clone()
            .can_save(proto::CanSaveRequest {
                blob_id: blob_id.to_be_bytes().to_vec(),
                part_id: part_id.to_be_bytes().to_vec(),
            })
            .await
            .map_err(grpc::from_status)?;
        Ok(())
    }
    async fn save_part(
        &self,
        blob_id: BlobId,
        stream_id: StreamId,
        length: u64,
        part_id: PartId,
    ) -> Result<(), Error> {
        self.client
            .clone()
            .save_part(proto::SavePartRequest {
                blob_id: blob_id.to_be_bytes().to_vec(),
                stream_id: stream_id.to_be_bytes().to_vec(),
                length,
                part_id: part_id.to_be_bytes().to_vec(),
            })
            .await
            .map_err(grpc::from_status)?;
        Ok(())
    }
    async fn finalize(&self, blob_id: BlobId, parts: Vec<PartId>) -> Result<u64, Error> {
        Ok(self
            .client
            .clone()
            .finalize(proto::FinalizeRequest {
                blob_id: blob_id.to_be_bytes().to_vec(),
                part_ids: parts
                    .into_iter()
                    .map(|id| id.to_be_bytes().to_vec())
                    .collect(),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner()
            .length)
    }
    async fn read(
        &self,
        blob_id: BlobId,
        range: std::ops::Range<u64>,
    ) -> Result<Self::StreamSliceStream, Error> {
        use futures_util::{StreamExt, stream};
        let slices = self
            .client
            .clone()
            .read(proto::ReadRequest {
                blob_id: blob_id.to_be_bytes().to_vec(),
                start: range.start,
                end: range.end,
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        Ok(Box::pin(
            stream::unfold(Some(slices), |state| async move {
                let mut slices = state?;
                let item = slices
                    .next()
                    .await?
                    .map_err(grpc::from_status)
                    .and_then(|slice| {
                        let stream_id = grpc::decode_id(&slice.stream_id)
                            .map_err(|e| Error::Rpc(Status::data_loss(e.message())))?;
                        Ok(StreamSlice {
                            stream_id,
                            skip: slice.skip,
                            length: slice.length,
                        })
                    });
                let next = if item.is_ok() { Some(slices) } else { None };
                Some((item, next))
            })
            .fuse(),
        ))
    }
}

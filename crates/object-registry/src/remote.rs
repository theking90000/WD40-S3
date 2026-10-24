//! Connected gRPC client. Failed operations are not replayed.
use crate::{
    BlobId, Cursor, DeleteResult, Error, Metadata, PageResult, RecordMetadata, StoreResult,
    VersionId,
    grpc::{self, proto},
    traits::ObjectRegistry,
};
use tonic::{Status, transport::Endpoint};
/// A connected registry client. Clones share its HTTP/2 channel.
#[derive(Clone)]
pub struct RemoteRegistry {
    client: proto::object_registry_client::ObjectRegistryClient<tonic::transport::Channel>,
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
            client: proto::object_registry_client::ObjectRegistryClient::new(channel)
                .max_decoding_message_size(grpc::MAX_MESSAGE_SIZE)
                .max_encoding_message_size(grpc::MAX_MESSAGE_SIZE),
        })
    }
}

impl ObjectRegistry for RemoteRegistry {
    async fn store(
        &self,
        key: &str,
        blob_id: BlobId,
        metadata: Metadata,
    ) -> Result<StoreResult, Error> {
        let result = self
            .client
            .clone()
            .store(proto::StoreRequest {
                key: key.to_owned(),
                blob_id: blob_id.to_be_bytes().to_vec(),
                metadata,
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        let previous = result
            .previous
            .map(grpc::decode_record)
            .transpose()
            .map_err(|error| Error::Rpc(Status::data_loss(error.message())))?;
        Ok(StoreResult {
            version_id: result.version_id,
            previous,
        })
    }
    async fn head(
        &self,
        key: &str,
        version_id: Option<VersionId>,
    ) -> Result<RecordMetadata, Error> {
        let record = self
            .client
            .clone()
            .head(proto::HeadRequest {
                key: key.to_owned(),
                version_id,
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        grpc::decode_record(record).map_err(|e| Error::Rpc(Status::data_loss(e.message())))
    }
    async fn list(
        &self,
        prefix: &str,
        cursor: Option<&Cursor>,
        limit: Option<usize>,
        include_versions: bool,
    ) -> Result<PageResult, Error> {
        let cursor = cursor.map(postcard::to_allocvec).transpose()?;
        let limit = limit
            .map(u64::try_from)
            .transpose()
            .map_err(|_| Error::InvalidLimit)?;
        let page = self
            .client
            .clone()
            .list(proto::ListRequest {
                prefix: prefix.to_owned(),
                cursor,
                limit,
                include_versions,
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        let decoded = (|| -> Result<_, Status> {
            let records = page
                .records
                .into_iter()
                .map(grpc::decode_record)
                .collect::<Result<Vec<_>, _>>()?;
            let next_cursor = page
                .next_cursor
                .as_deref()
                .map(grpc::decode_postcard)
                .transpose()?;
            Ok(PageResult {
                records,
                next_cursor,
            })
        })();
        decoded.map_err(|e| Error::Rpc(Status::data_loss(e.message())))
    }
    async fn delete(
        &self,
        key: &str,
        version_id: Option<VersionId>,
    ) -> Result<DeleteResult, Error> {
        let result = self
            .client
            .clone()
            .delete(proto::HeadRequest {
                key: key.to_owned(),
                version_id,
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        Ok(DeleteResult {
            version_id: result.version_id,
            previous: result
                .previous
                .map(grpc::decode_record)
                .transpose()
                .map_err(|error| Error::Rpc(Status::data_loss(error.message())))?,
            current: result
                .current
                .map(grpc::decode_record)
                .transpose()
                .map_err(|error| Error::Rpc(Status::data_loss(error.message())))?,
        })
    }
}

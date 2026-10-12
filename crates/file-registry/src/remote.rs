//! Connected gRPC client. Failed operations are not replayed.
use crate::{
    Error, FileAccess, FileId, ReadAccessPath, WriteAccessPath,
    grpc::{self, proto},
    traits::FileRegistry,
};
use tonic::{Status, transport::Endpoint};
/// A connected registry client. Clones share its HTTP/2 channel.
#[derive(Clone)]
pub struct RemoteRegistry {
    client: proto::file_registry_client::FileRegistryClient<tonic::transport::Channel>,
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
            client: proto::file_registry_client::FileRegistryClient::new(channel)
                .max_decoding_message_size(grpc::MAX_MESSAGE_SIZE)
                .max_encoding_message_size(grpc::MAX_MESSAGE_SIZE),
        })
    }
}

impl FileRegistry for RemoteRegistry {
    type WriteAccessPathStream = <crate::LocalRegistry as FileRegistry>::WriteAccessPathStream;
    type ReadAccessPathStream = <crate::LocalRegistry as FileRegistry>::ReadAccessPathStream;
    async fn create(
        &self,
        count: usize,
        max_file_size: u64,
    ) -> Result<Self::WriteAccessPathStream, Error> {
        let count_u64 = u64::try_from(count)
            .map_err(|_| Error::Rpc(Status::invalid_argument("count exceeds u64")))?;
        let response = self
            .client
            .clone()
            .create(proto::CreateRequest {
                count: count_u64,
                max_file_size,
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        if response.paths.len() != count {
            return Err(Error::Rpc(Status::data_loss(
                "registry returned the wrong number of write paths",
            )));
        }
        let paths = response
            .paths
            .into_iter()
            .map(|p| {
                Ok(WriteAccessPath {
                    id: grpc::decode_id(&p.id)?,
                    access_path: grpc::decode_postcard(&p.access_path)?,
                })
            })
            .collect::<Result<Vec<_>, Status>>()
            .map_err(|e| Error::Rpc(Status::data_loss(e.message())))?;
        // The public item stream cannot carry errors: receive and validate it first.
        Ok(futures_util::stream::iter(paths))
    }
    async fn store(&self, files: &[FileAccess]) -> Result<(), Error> {
        let files = files
            .iter()
            .map(|f| {
                Ok(proto::FileAccess {
                    id: f.id.to_be_bytes().to_vec(),
                    access_path: postcard::to_allocvec(&f.access_path)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        self.client
            .clone()
            .store(proto::StoreRequest { files })
            .await
            .map_err(grpc::from_status)?;
        Ok(())
    }
    async fn retrieve(&self, ids: &[FileId]) -> Result<Self::ReadAccessPathStream, Error> {
        use futures_util::{StreamExt, stream};
        let paths = self
            .client
            .clone()
            .retrieve(proto::RetrieveRequest {
                ids: ids.iter().map(|id| id.to_be_bytes().to_vec()).collect(),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        Ok(Box::pin(
            stream::unfold(Some(paths), |state| async move {
                let mut paths = state?;
                let item = paths
                    .next()
                    .await?
                    .map_err(grpc::from_status)
                    .and_then(|p| {
                        let decoded = (|| -> Result<_, Status> {
                            Ok(ReadAccessPath {
                                id: grpc::decode_id(&p.id)?,
                                access_path: grpc::decode_postcard(&p.access_path)?,
                            })
                        })();
                        decoded.map_err(|e| Error::Rpc(Status::data_loss(e.message())))
                    });
                let next = if item.is_ok() { Some(paths) } else { None };
                Some((item, next))
            })
            .fuse(),
        ))
    }
}

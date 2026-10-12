//! Versioned gRPC service delegating to the injected registry enum.
use crate::Error;
use prost::Message;
use tonic::{Code, Status};

/// Generated protocol v1 messages and feature-gated service stubs.
pub mod proto {
    tonic::include_proto!("wd40.file_registry.v1");
}
/// Shared per-message encoding and decoding limit.
pub const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;
pub(crate) fn decode_id(bytes: &[u8]) -> Result<u128, Status> {
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("registry IDs must contain exactly 16 bytes"))?;
    Ok(u128::from_be_bytes(bytes))
}
pub(crate) fn decode_postcard<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Status> {
    let (value, tail) = postcard::take_from_bytes(bytes)
        .map_err(|_| Status::invalid_argument("invalid v1 Postcard payload"))?;
    if !tail.is_empty() {
        return Err(Status::invalid_argument("trailing Postcard bytes"));
    }
    Ok(value)
}

#[cfg(feature = "grpc-server")]
#[derive(Clone)]
/// Delegates to an injected Local or Remote registry without inspecting the variant.
pub struct GrpcService {
    registry: crate::FileRegistry,
}
#[cfg(feature = "grpc-server")]
impl GrpcService {
    /// Injects the registry served by this adapter.
    pub fn new(registry: crate::FileRegistry) -> Self {
        Self { registry }
    }
    /// Builds the Tonic service with the shared 4 MiB limits.
    pub fn into_server(self) -> proto::file_registry_server::FileRegistryServer<Self> {
        proto::file_registry_server::FileRegistryServer::new(self)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE)
    }
}

#[cfg(feature = "grpc-server")]
pub(crate) fn into_status(error: Error) -> Status {
    use proto::registry_error::Kind;
    let message = error.to_string();
    let (code, details) = match error {
        Error::FileNotFound(id) => (
            Code::NotFound,
            proto::RegistryError {
                kind: Kind::FileNotFound as i32,
                id: id.to_be_bytes().to_vec(),
            },
        ),
        Error::FilePending(id) => (
            Code::FailedPrecondition,
            proto::RegistryError {
                kind: Kind::FilePending as i32,
                id: id.to_be_bytes().to_vec(),
            },
        ),
        Error::MissingReadAccessPath(id) => (
            Code::NotFound,
            proto::RegistryError {
                kind: Kind::MissingReadAccessPath as i32,
                id: id.to_be_bytes().to_vec(),
            },
        ),
        Error::MissingWriteAccessPath => (
            Code::ResourceExhausted,
            proto::RegistryError {
                kind: Kind::MissingWriteAccessPath as i32,
                ..Default::default()
            },
        ),
        Error::Rpc(status) => return status,
        #[cfg(feature = "grpc-client")]
        Error::Transport(_) => return Status::unavailable("upstream registry transport failed"),
        _ => return Status::internal("registry storage or backend operation failed"),
    };
    Status::with_details(code, message, details.encode_to_vec().into())
}
#[cfg(feature = "grpc-client")]
pub(crate) fn from_status(status: Status) -> Error {
    use proto::registry_error::Kind;
    let grpc_id = |bytes: &[u8]| decode_id(bytes).ok();
    let decoded = (|| -> Option<Error> {
        let d = proto::RegistryError::decode(status.details()).ok()?;
        Some(match (status.code(), Kind::try_from(d.kind).ok()?) {
            (Code::NotFound, Kind::FileNotFound) => Error::FileNotFound(grpc_id(&d.id)?),
            (Code::FailedPrecondition, Kind::FilePending) => Error::FilePending(grpc_id(&d.id)?),
            (Code::NotFound, Kind::MissingReadAccessPath) => {
                Error::MissingReadAccessPath(grpc_id(&d.id)?)
            }
            (Code::ResourceExhausted, Kind::MissingWriteAccessPath) => {
                Error::MissingWriteAccessPath
            }
            _ => return None,
        })
    })();
    match decoded {
        Some(error) => error,
        None => Error::Rpc(status),
    }
}

#[cfg(feature = "grpc-server")]
#[tonic::async_trait]
impl proto::file_registry_server::FileRegistry for GrpcService {
    type RetrieveStream = std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<proto::ReadAccessPath, Status>> + Send>,
    >;
    async fn create(
        &self,
        request: tonic::Request<proto::CreateRequest>,
    ) -> Result<tonic::Response<proto::CreateResponse>, Status> {
        use crate::traits::FileRegistry as _;
        use futures_util::StreamExt;
        let r = request.into_inner();
        let count = usize::try_from(r.count)
            .map_err(|_| Status::invalid_argument("count exceeds usize"))?;
        if count > MAX_MESSAGE_SIZE / 16 {
            return Err(Status::out_of_range(
                "count exceeds 4 MiB response ID budget",
            ));
        }
        let mut allocated = self
            .registry
            .create(count, r.max_file_size)
            .await
            .map_err(into_status)?;
        let mut paths = Vec::new();
        while let Some(path) = allocated.next().await {
            paths.push(proto::WriteAccessPath {
                id: path.id.to_be_bytes().to_vec(),
                access_path: postcard::to_allocvec(&path.access_path)
                    .map_err(|e| into_status(Error::Serialization(e)))?,
            });
        }
        Ok(tonic::Response::new(proto::CreateResponse { paths }))
    }
    async fn store(
        &self,
        request: tonic::Request<proto::StoreRequest>,
    ) -> Result<tonic::Response<proto::Empty>, Status> {
        use crate::traits::FileRegistry as _;
        let files = request
            .into_inner()
            .files
            .into_iter()
            .map(|f| {
                Ok(crate::FileAccess {
                    id: decode_id(&f.id)?,
                    access_path: decode_postcard(&f.access_path)?,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;
        self.registry.store(&files).await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::Empty {}))
    }
    async fn retrieve(
        &self,
        request: tonic::Request<proto::RetrieveRequest>,
    ) -> Result<tonic::Response<Self::RetrieveStream>, Status> {
        use crate::traits::FileRegistry as _;
        use futures_util::{StreamExt, stream};
        let ids = request
            .into_inner()
            .ids
            .iter()
            .map(|id| decode_id(id))
            .collect::<Result<Vec<_>, _>>()?;
        let paths = self.registry.retrieve(&ids).await.map_err(into_status)?;
        let response = stream::unfold(Some(paths), |state| async move {
            let mut paths = state?;
            let item = paths.next().await?.map_err(into_status).and_then(|path| {
                Ok(proto::ReadAccessPath {
                    id: path.id.to_be_bytes().to_vec(),
                    access_path: postcard::to_allocvec(&path.access_path)
                        .map_err(|e| into_status(Error::Serialization(e)))?,
                })
            });
            let next = if item.is_ok() { Some(paths) } else { None };
            Some((item, next))
        })
        .fuse();
        Ok(tonic::Response::new(Box::pin(response)))
    }
}

#[cfg(all(test, feature = "grpc-client", feature = "grpc-server"))]
mod tests {
    use super::*;
    #[test]
    fn domain_details_and_unknown_statuses_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        for original in [
            Error::FileNotFound(u128::MAX),
            Error::FilePending(u128::MAX),
            Error::MissingReadAccessPath(u128::MAX),
            Error::MissingWriteAccessPath,
        ] {
            let kind = std::mem::discriminant(&original);
            let message = original.to_string();
            let decoded = from_status(into_status(original));
            if std::mem::discriminant(&decoded) != kind || decoded.to_string() != message {
                return Err("domain error variant or fields changed".into());
            }
        }
        for status in [
            Status::unavailable("upstream unavailable"),
            Status::with_details(Code::NotFound, "malformed details", vec![0xff].into()),
        ] {
            let code = status.code();
            let message = status.message().to_owned();
            let details = status.details().to_vec();
            match from_status(status) {
                Error::Rpc(status)
                    if status.code() == code
                        && status.message() == message
                        && status.details() == details => {}
                _ => return Err("unknown RPC status was changed".into()),
            }
        }
        Ok(())
    }
}

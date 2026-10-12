//! Versioned gRPC protocol and server adapter. Metadata contains secret keys.

use prost::Message;
use tonic::{Code, Status};

use crate::{Error, Metadata};

/// Generated v1 messages, client and server, according to enabled features.
pub mod proto {
    tonic::include_proto!("wd40.stream_registry.v1");
}

/// Per-message encoding and decoding limit, shared by clients and servers.
pub const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

pub(crate) fn decode_id(bytes: &[u8]) -> Result<u128, Status> {
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("registry IDs must contain exactly 16 bytes"))?;
    Ok(u128::from_be_bytes(bytes))
}

pub(crate) fn decode_metadata(bytes: &[u8]) -> Result<Metadata, Status> {
    let (metadata, remaining) = postcard::take_from_bytes(bytes)
        .map_err(|_| Status::invalid_argument("invalid CRAFT v1 metadata"))?;
    if !remaining.is_empty() {
        return Err(Status::invalid_argument("trailing CRAFT metadata bytes"));
    }
    Ok(metadata)
}

#[cfg(feature = "grpc-server")]
pub(crate) fn into_status(error: Error) -> Status {
    use proto::registry_error::Kind;
    let message = error.to_string();
    let (code, kind, id) = match error {
        Error::StreamNotFound(id) => (Code::NotFound, Kind::StreamNotFound, Some(id)),
        Error::StreamPartNotFound(id) => (Code::NotFound, Kind::StreamPartNotFound, Some(id)),
        Error::StreamAlreadyExists(id) => {
            (Code::AlreadyExists, Kind::StreamAlreadyExists, Some(id))
        }
        Error::StreamPartAlreadyExists(id) => {
            (Code::AlreadyExists, Kind::StreamPartAlreadyExists, Some(id))
        }
        Error::LengthOverflow => (Code::OutOfRange, Kind::LengthOverflow, None),
        Error::Rpc(status) => return status,
        #[cfg(feature = "grpc-client")]
        Error::Transport(_) => return Status::unavailable("upstream registry transport failed"),
        Error::Database(_)
        | Error::Serialization(_)
        | Error::FlushWorker(_)
        | Error::FlushCanceled(_) => {
            return Status::internal("stream registry storage or serialization failed");
        }
    };
    let details = proto::RegistryError {
        kind: kind as i32,
        id: id.map_or_else(Vec::new, |id| id.to_be_bytes().to_vec()),
    };
    Status::with_details(code, message, details.encode_to_vec().into())
}

#[cfg(feature = "grpc-client")]
pub(crate) fn from_status(status: Status) -> Error {
    use proto::registry_error::Kind;
    let decoded = proto::RegistryError::decode(status.details()).ok();
    if let Some(details) = decoded {
        let id = decode_id(&details.id).ok();
        match (status.code(), Kind::try_from(details.kind), id) {
            (Code::NotFound, Ok(Kind::StreamNotFound), Some(id)) => {
                return Error::StreamNotFound(id);
            }
            (Code::NotFound, Ok(Kind::StreamPartNotFound), Some(id)) => {
                return Error::StreamPartNotFound(id);
            }
            (Code::AlreadyExists, Ok(Kind::StreamAlreadyExists), Some(id)) => {
                return Error::StreamAlreadyExists(id);
            }
            (Code::AlreadyExists, Ok(Kind::StreamPartAlreadyExists), Some(id)) => {
                return Error::StreamPartAlreadyExists(id);
            }
            (Code::OutOfRange, Ok(Kind::LengthOverflow), _) if details.id.is_empty() => {
                return Error::LengthOverflow;
            }
            _ => {}
        }
    }
    Error::Rpc(status)
}

/// Serves any injected registry, including one pointing at another gRPC server.
#[cfg(feature = "grpc-server")]
#[derive(Clone)]
pub struct GrpcService {
    registry: crate::StreamRegistry,
}

#[cfg(feature = "grpc-server")]
impl GrpcService {
    /// Injects a registry without restricting its implementation.
    pub fn new(registry: crate::StreamRegistry) -> Self {
        Self { registry }
    }

    /// Wraps this adapter with the shared encoding and decoding limits.
    pub fn into_server(self) -> proto::stream_registry_server::StreamRegistryServer<Self> {
        proto::stream_registry_server::StreamRegistryServer::new(self)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE)
    }
}

#[cfg(all(test, feature = "grpc-client", feature = "grpc-server"))]
mod tests {
    use super::*;

    #[test]
    fn domain_details_and_unrecognized_statuses_roundtrip() -> Result<(), Box<dyn std::error::Error>>
    {
        let cases = [
            Error::StreamNotFound(u128::MAX),
            Error::StreamPartNotFound(u128::MAX),
            Error::StreamAlreadyExists(u128::MAX),
            Error::StreamPartAlreadyExists(u128::MAX),
            Error::LengthOverflow,
        ];
        for original in cases {
            let kind = std::mem::discriminant(&original);
            let message = original.to_string();
            let decoded = from_status(into_status(original));
            if std::mem::discriminant(&decoded) != kind || decoded.to_string() != message {
                return Err("domain error variant or ID changed".into());
            }
        }
        for status in [
            Status::unavailable("upstream unavailable"),
            Status::with_details(Code::NotFound, "unknown detail", vec![0xff].into()),
            Status::with_details(
                Code::Internal,
                "wrong code",
                proto::RegistryError {
                    kind: proto::registry_error::Kind::StreamNotFound as i32,
                    id: 42_u128.to_be_bytes().to_vec(),
                }
                .encode_to_vec()
                .into(),
            ),
        ] {
            let expected_code = status.code();
            let expected_message = status.message().to_owned();
            let expected_details = status.details().to_vec();
            match from_status(status) {
                Error::Rpc(status)
                    if status.code() == expected_code
                        && status.message() == expected_message
                        && status.details() == expected_details => {}
                _ => return Err("unrecognized status was changed".into()),
            }
        }
        Ok(())
    }
}

#[cfg(feature = "grpc-server")]
#[tonic::async_trait]
impl proto::stream_registry_server::StreamRegistry for GrpcService {
    type RetrievePartsStream = std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<proto::StreamPart, Status>> + Send>,
    >;

    async fn insert(
        &self,
        request: tonic::Request<proto::InsertRequest>,
    ) -> Result<tonic::Response<proto::InsertResponse>, Status> {
        use crate::traits::StreamRegistry as _;
        let request = request.into_inner();
        let id = self
            .registry
            .insert(
                decode_id(&request.file_id)?,
                decode_metadata(&request.metadata)?,
            )
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::InsertResponse {
            part_id: id.to_be_bytes().to_vec(),
        }))
    }

    async fn store(
        &self,
        request: tonic::Request<proto::StoreRequest>,
    ) -> Result<tonic::Response<proto::StoreResponse>, Status> {
        use crate::traits::StreamRegistry as _;
        let parts = request
            .into_inner()
            .part_ids
            .iter()
            .map(|id| decode_id(id))
            .collect::<Result<Vec<_>, _>>()?;
        let id = self.registry.store(parts).await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::StoreResponse {
            stream_id: id.to_be_bytes().to_vec(),
        }))
    }

    async fn retrieve_parts(
        &self,
        request: tonic::Request<proto::RetrievePartsRequest>,
    ) -> Result<tonic::Response<Self::RetrievePartsStream>, Status> {
        use crate::traits::StreamRegistry as _;
        use futures_util::{StreamExt, stream};
        let ids = request
            .into_inner()
            .stream_ids
            .iter()
            .map(|id| decode_id(id))
            .collect::<Result<Vec<_>, _>>()?;
        let parts = self
            .registry
            .retrieve_parts(&ids)
            .await
            .map_err(into_status)?;
        let response = stream::unfold(Some(parts), |state| async move {
            let mut parts = state?;
            let part = parts.next().await?;
            let item = part.map_err(into_status).and_then(|part| {
                Ok(proto::StreamPart {
                    file_id: part.file_id.to_be_bytes().to_vec(),
                    metadata: postcard::to_allocvec(&part.metadata)
                        .map_err(|error| into_status(Error::Serialization(error)))?,
                })
            });
            let state = if item.is_ok() { Some(parts) } else { None };
            Some((item, state))
        });
        Ok(tonic::Response::new(Box::pin(response.fuse())))
    }
}

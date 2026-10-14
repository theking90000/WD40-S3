//! Versioned gRPC service delegating to the injected registry enum.
use crate::Error;
use prost::Message;
use tonic::{Code, Status};

/// Generated protocol v1 messages and feature-gated service stubs.
pub mod proto {
    tonic::include_proto!("wd40.blob_registry.v1");
}
/// Shared per-message encoding and decoding limit.
pub const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;
pub(crate) fn decode_id(bytes: &[u8]) -> Result<u128, Status> {
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("registry IDs must contain exactly 16 bytes"))?;
    Ok(u128::from_be_bytes(bytes))
}

#[cfg(feature = "grpc-server")]
#[derive(Clone)]
/// Delegates to an injected Local or Remote registry without inspecting the variant.
pub struct GrpcService {
    registry: crate::BlobRegistry,
}
#[cfg(feature = "grpc-server")]
impl GrpcService {
    /// Injects the registry served by this adapter.
    pub fn new(registry: crate::BlobRegistry) -> Self {
        Self { registry }
    }
    /// Builds the Tonic service with the shared 4 MiB limits.
    pub fn into_server(self) -> proto::blob_registry_server::BlobRegistryServer<Self> {
        proto::blob_registry_server::BlobRegistryServer::new(self)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE)
    }
}

#[cfg(feature = "grpc-server")]
pub(crate) fn into_status(error: Error) -> Status {
    use proto::registry_error::Kind;
    let message = error.to_string();
    let (code, details) = match error {
        Error::BlobNotFound(blob_id) => (
            Code::NotFound,
            proto::RegistryError {
                kind: Kind::BlobNotFound as i32,
                blob_id: blob_id.to_be_bytes().to_vec(),
                ..Default::default()
            },
        ),
        Error::BlobAlreadyExists(blob_id) => (
            Code::AlreadyExists,
            proto::RegistryError {
                kind: Kind::BlobAlreadyExists as i32,
                blob_id: blob_id.to_be_bytes().to_vec(),
                ..Default::default()
            },
        ),
        Error::BlobNotFinalized(blob_id) => (
            Code::FailedPrecondition,
            proto::RegistryError {
                kind: Kind::BlobNotFinalized as i32,
                blob_id: blob_id.to_be_bytes().to_vec(),
                ..Default::default()
            },
        ),
        Error::BlobFinalized(blob_id) => (
            Code::FailedPrecondition,
            proto::RegistryError {
                kind: Kind::BlobFinalized as i32,
                blob_id: blob_id.to_be_bytes().to_vec(),
                ..Default::default()
            },
        ),
        Error::PartNotFound { blob_id, part_id } => (
            Code::NotFound,
            proto::RegistryError {
                kind: Kind::PartNotFound as i32,
                blob_id: blob_id.to_be_bytes().to_vec(),
                part_id: part_id.to_be_bytes().to_vec(),
                ..Default::default()
            },
        ),
        Error::InvalidRange { start, end, length } => (
            Code::OutOfRange,
            proto::RegistryError {
                kind: Kind::InvalidRange as i32,
                start,
                end,
                length,
                ..Default::default()
            },
        ),
        Error::LengthOverflow => (
            Code::OutOfRange,
            proto::RegistryError {
                kind: Kind::LengthOverflow as i32,
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
            (Code::NotFound, Kind::BlobNotFound) => Error::BlobNotFound(grpc_id(&d.blob_id)?),
            (Code::AlreadyExists, Kind::BlobAlreadyExists) => {
                Error::BlobAlreadyExists(grpc_id(&d.blob_id)?)
            }
            (Code::FailedPrecondition, Kind::BlobNotFinalized) => {
                Error::BlobNotFinalized(grpc_id(&d.blob_id)?)
            }
            (Code::FailedPrecondition, Kind::BlobFinalized) => {
                Error::BlobFinalized(grpc_id(&d.blob_id)?)
            }
            (Code::NotFound, Kind::PartNotFound) => Error::PartNotFound {
                blob_id: grpc_id(&d.blob_id)?,
                part_id: grpc_id(&d.part_id)?,
            },
            (Code::OutOfRange, Kind::InvalidRange) => Error::InvalidRange {
                start: d.start,
                end: d.end,
                length: d.length,
            },
            (Code::OutOfRange, Kind::LengthOverflow) => Error::LengthOverflow,
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
impl proto::blob_registry_server::BlobRegistry for GrpcService {
    type ReadStream = std::pin::Pin<
        Box<dyn futures_core::Stream<Item = Result<proto::StreamSlice, Status>> + Send>,
    >;
    async fn create(
        &self,
        _: tonic::Request<proto::Empty>,
    ) -> Result<tonic::Response<proto::CreateResponse>, Status> {
        use crate::traits::BlobRegistry as _;
        let id = self.registry.create().await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::CreateResponse {
            blob_id: id.to_be_bytes().to_vec(),
        }))
    }
    async fn can_save(
        &self,
        request: tonic::Request<proto::CanSaveRequest>,
    ) -> Result<tonic::Response<proto::Empty>, Status> {
        use crate::traits::BlobRegistry as _;
        let r = request.into_inner();
        self.registry
            .can_save(decode_id(&r.blob_id)?)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::Empty {}))
    }
    async fn save_part(
        &self,
        request: tonic::Request<proto::SavePartRequest>,
    ) -> Result<tonic::Response<proto::Empty>, Status> {
        use crate::traits::BlobRegistry as _;
        let r = request.into_inner();
        self.registry
            .save_part(
                decode_id(&r.blob_id)?,
                decode_id(&r.stream_id)?,
                r.length,
                decode_id(&r.part_id)?,
            )
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::Empty {}))
    }
    async fn get_parts(
        &self,
        request: tonic::Request<proto::GetPartsRequest>,
    ) -> Result<tonic::Response<proto::GetPartsResponse>, Status> {
        use crate::traits::BlobRegistry as _;
        let r = request.into_inner();
        let parts = self
            .registry
            .get_parts(decode_id(&r.blob_id)?)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::GetPartsResponse {
            parts: parts
                .into_iter()
                .map(|(part_id, part)| proto::BlobPart {
                    part_id: part_id.to_be_bytes().to_vec(),
                    stream_id: part.stream_id.to_be_bytes().to_vec(),
                    length: part.length,
                })
                .collect(),
        }))
    }
    async fn finalize(
        &self,
        request: tonic::Request<proto::FinalizeRequest>,
    ) -> Result<tonic::Response<proto::FinalizeResponse>, Status> {
        use crate::traits::BlobRegistry as _;
        let r = request.into_inner();
        let parts = r
            .part_ids
            .iter()
            .map(|id| decode_id(id))
            .collect::<Result<Vec<_>, _>>()?;
        let length = self
            .registry
            .finalize(decode_id(&r.blob_id)?, parts)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::FinalizeResponse { length }))
    }
    async fn read(
        &self,
        request: tonic::Request<proto::ReadRequest>,
    ) -> Result<tonic::Response<Self::ReadStream>, Status> {
        use crate::traits::BlobRegistry as _;
        use futures_util::{StreamExt, stream};
        let r = request.into_inner();
        let slices = self
            .registry
            .read(decode_id(&r.blob_id)?, r.start..r.end)
            .await
            .map_err(into_status)?;
        let response = stream::unfold(Some(slices), |state| async move {
            let mut slices = state?;
            let item = slices
                .next()
                .await?
                .map_err(into_status)
                .map(|slice| proto::StreamSlice {
                    stream_id: slice.stream_id.to_be_bytes().to_vec(),
                    skip: slice.skip,
                    length: slice.length,
                });
            let next = if item.is_ok() { Some(slices) } else { None };
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
            Error::BlobNotFound(u128::MAX),
            Error::BlobAlreadyExists(u128::MAX),
            Error::BlobNotFinalized(u128::MAX),
            Error::BlobFinalized(u128::MAX),
            Error::PartNotFound {
                blob_id: u128::MAX,
                part_id: u128::MAX,
            },
            Error::InvalidRange {
                start: 1,
                end: 4,
                length: 3,
            },
            Error::LengthOverflow,
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

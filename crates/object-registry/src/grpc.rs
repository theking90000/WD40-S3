//! Versioned gRPC service delegating to the injected registry enum.
use crate::Error;
use prost::Message;
use tonic::{Code, Status};

/// Generated protocol v1 messages and feature-gated service stubs.
pub mod proto {
    tonic::include_proto!("wd40.object_registry.v1");
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
    registry: crate::ObjectRegistry,
}
#[cfg(feature = "grpc-server")]
impl GrpcService {
    /// Injects the registry served by this adapter.
    pub fn new(registry: crate::ObjectRegistry) -> Self {
        Self { registry }
    }
    /// Builds the Tonic service with the shared 4 MiB limits.
    pub fn into_server(self) -> proto::object_registry_server::ObjectRegistryServer<Self> {
        proto::object_registry_server::ObjectRegistryServer::new(self)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE)
    }
}

#[cfg(feature = "grpc-server")]
pub(crate) fn into_status(error: Error) -> Status {
    use proto::registry_error::Kind;
    let message = error.to_string();
    let (code, details) = match error {
        Error::InvalidKey => (
            Code::InvalidArgument,
            proto::RegistryError {
                kind: Kind::InvalidKey as i32,
                ..Default::default()
            },
        ),
        Error::InvalidPrefix => (
            Code::InvalidArgument,
            proto::RegistryError {
                kind: Kind::InvalidPrefix as i32,
                ..Default::default()
            },
        ),
        Error::InvalidLimit => (
            Code::InvalidArgument,
            proto::RegistryError {
                kind: Kind::InvalidLimit as i32,
                ..Default::default()
            },
        ),
        Error::InvalidCursor => (
            Code::InvalidArgument,
            proto::RegistryError {
                kind: Kind::InvalidCursor as i32,
                ..Default::default()
            },
        ),
        Error::ObjectNotFound(key) => (
            Code::NotFound,
            proto::RegistryError {
                kind: Kind::ObjectNotFound as i32,
                key,
                ..Default::default()
            },
        ),
        Error::VersionNotFound { key, version_id } => (
            Code::NotFound,
            proto::RegistryError {
                kind: Kind::VersionNotFound as i32,
                key,
                version_id,
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
    let decoded = (|| -> Option<Error> {
        let d = proto::RegistryError::decode(status.details()).ok()?;
        Some(match (status.code(), Kind::try_from(d.kind).ok()?) {
            (Code::InvalidArgument, Kind::InvalidKey) => Error::InvalidKey,
            (Code::InvalidArgument, Kind::InvalidPrefix) => Error::InvalidPrefix,
            (Code::InvalidArgument, Kind::InvalidLimit) => Error::InvalidLimit,
            (Code::InvalidArgument, Kind::InvalidCursor) => Error::InvalidCursor,
            (Code::NotFound, Kind::ObjectNotFound) => Error::ObjectNotFound(d.key),
            (Code::NotFound, Kind::VersionNotFound) => Error::VersionNotFound {
                key: d.key,
                version_id: d.version_id,
            },
            _ => return None,
        })
    })();
    match decoded {
        Some(error) => error,
        None => Error::Rpc(status),
    }
}

#[cfg(feature = "grpc-server")]
fn encode_record(record: crate::RecordMetadata) -> proto::RecordMetadata {
    proto::RecordMetadata {
        key: record.key,
        version_id: record.version_id,
        blob_id: record.blob_id.map(|id| id.to_be_bytes().to_vec()),
        metadata: record.metadata,
        is_delete_marker: record.is_delete_marker,
    }
}
#[cfg(feature = "grpc-client")]
pub(crate) fn decode_record(
    record: proto::RecordMetadata,
) -> Result<crate::RecordMetadata, Status> {
    Ok(crate::RecordMetadata {
        key: record.key,
        version_id: record.version_id,
        blob_id: record.blob_id.as_deref().map(decode_id).transpose()?,
        metadata: record.metadata,
        is_delete_marker: record.is_delete_marker,
    })
}
#[cfg(feature = "grpc-server")]
#[tonic::async_trait]
impl proto::object_registry_server::ObjectRegistry for GrpcService {
    async fn store(
        &self,
        request: tonic::Request<proto::StoreRequest>,
    ) -> Result<tonic::Response<proto::VersionResponse>, Status> {
        use crate::traits::ObjectRegistry as _;
        let r = request.into_inner();
        let version_id = self
            .registry
            .store(&r.key, decode_id(&r.blob_id)?, r.metadata)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::VersionResponse { version_id }))
    }
    async fn head(
        &self,
        request: tonic::Request<proto::HeadRequest>,
    ) -> Result<tonic::Response<proto::RecordMetadata>, Status> {
        use crate::traits::ObjectRegistry as _;
        let r = request.into_inner();
        let record = self
            .registry
            .head(&r.key, r.version_id)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(encode_record(record)))
    }
    async fn list(
        &self,
        request: tonic::Request<proto::ListRequest>,
    ) -> Result<tonic::Response<proto::PageResult>, Status> {
        use crate::traits::ObjectRegistry as _;
        let r = request.into_inner();
        let cursor = r
            .cursor
            .as_deref()
            .map(decode_postcard::<crate::Cursor>)
            .transpose()
            .map_err(|_| into_status(Error::InvalidCursor))?;
        let limit = r
            .limit
            .map(usize::try_from)
            .transpose()
            .map_err(|_| into_status(Error::InvalidLimit))?;
        let page = self
            .registry
            .list(&r.prefix, cursor.as_ref(), limit, r.include_versions)
            .await
            .map_err(into_status)?;
        let next_cursor = page
            .next_cursor
            .as_ref()
            .map(postcard::to_allocvec)
            .transpose()
            .map_err(|e| into_status(Error::Serialization(e)))?;
        Ok(tonic::Response::new(proto::PageResult {
            records: page.records.into_iter().map(encode_record).collect(),
            next_cursor,
        }))
    }
    async fn delete(
        &self,
        request: tonic::Request<proto::HeadRequest>,
    ) -> Result<tonic::Response<proto::VersionResponse>, Status> {
        use crate::traits::ObjectRegistry as _;
        let r = request.into_inner();
        let result = self
            .registry
            .delete(&r.key, r.version_id)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::VersionResponse {
            version_id: result.version_id,
        }))
    }
}

#[cfg(all(test, feature = "grpc-client", feature = "grpc-server"))]
mod tests {
    use super::*;
    #[test]
    fn domain_details_and_unknown_statuses_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        for original in [
            Error::InvalidKey,
            Error::InvalidPrefix,
            Error::InvalidLimit,
            Error::InvalidCursor,
            Error::ObjectNotFound("key".into()),
            Error::VersionNotFound {
                key: "key".into(),
                version_id: u64::MAX,
            },
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

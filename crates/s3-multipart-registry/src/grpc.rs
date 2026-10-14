//! Protocol v1 and a service delegating to the injected registry enum.
use prost::Message;
use tonic::{Code, Status};

#[cfg(feature = "grpc-server")]
use crate::MultipartUploadId;
use crate::{BlobId, Error, MultipartUpload};

pub mod proto {
    tonic::include_proto!("wd40.s3_multipart_registry.v1");
}

pub const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

pub(crate) fn decode_blob_id(bytes: &[u8]) -> Result<BlobId, Status> {
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("blob ID must contain exactly 16 bytes"))?;
    Ok(BlobId::from_be_bytes(bytes))
}

pub(crate) fn decode_upload(value: proto::MultipartUpload) -> Result<MultipartUpload, Status> {
    Ok(MultipartUpload {
        blob_id: decode_blob_id(&value.blob_id)?,
        bucket: value.bucket,
        key: value.key,
        metadata: value.metadata,
    })
}

pub(crate) fn encode_upload(value: MultipartUpload) -> proto::MultipartUpload {
    proto::MultipartUpload {
        blob_id: value.blob_id.to_be_bytes().to_vec(),
        bucket: value.bucket,
        key: value.key,
        metadata: value.metadata,
    }
}

#[cfg(feature = "grpc-server")]
pub(crate) fn into_status(error: Error) -> Status {
    match error {
        Error::BlobAlreadyExists(blob_id) => Status::with_details(
            Code::AlreadyExists,
            "blob already has a multipart upload",
            proto::RegistryError {
                kind: proto::registry_error::Kind::BlobAlreadyExists as i32,
                blob_id: blob_id.to_be_bytes().to_vec(),
            }
            .encode_to_vec()
            .into(),
        ),
        Error::InvalidUploadId => Status::invalid_argument("invalid upload ID"),
        Error::Rpc(status) => status,
        #[cfg(feature = "grpc-client")]
        Error::Transport(_) => Status::unavailable("upstream registry transport failed"),
        _ => Status::internal("registry storage operation failed"),
    }
}

#[cfg(feature = "grpc-client")]
pub(crate) fn from_status(status: Status) -> Error {
    if status.code() == Code::AlreadyExists
        && let Ok(details) = proto::RegistryError::decode(status.details())
        && details.kind == proto::registry_error::Kind::BlobAlreadyExists as i32
        && let Ok(blob_id) = decode_blob_id(&details.blob_id)
    {
        return Error::BlobAlreadyExists(blob_id);
    }
    Error::Rpc(status)
}

#[cfg(feature = "grpc-server")]
#[derive(Clone)]
pub struct GrpcService {
    registry: crate::S3MultipartRegistry,
}

#[cfg(feature = "grpc-server")]
impl GrpcService {
    pub fn new(registry: crate::S3MultipartRegistry) -> Self {
        Self { registry }
    }

    pub fn into_server(
        self,
    ) -> proto::s3_multipart_registry_server::S3MultipartRegistryServer<Self> {
        proto::s3_multipart_registry_server::S3MultipartRegistryServer::new(self)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE)
    }
}

#[cfg(feature = "grpc-server")]
#[tonic::async_trait]
impl proto::s3_multipart_registry_server::S3MultipartRegistry for GrpcService {
    async fn create(
        &self,
        request: tonic::Request<proto::CreateRequest>,
    ) -> Result<tonic::Response<proto::CreateResponse>, Status> {
        use crate::traits::S3MultipartRegistry as _;
        let upload = request
            .into_inner()
            .upload
            .ok_or_else(|| Status::invalid_argument("missing multipart upload"))?;
        let id = self
            .registry
            .create(decode_upload(upload)?)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::CreateResponse {
            upload_id: id.as_str().to_owned(),
        }))
    }

    async fn get(
        &self,
        request: tonic::Request<proto::UploadRequest>,
    ) -> Result<tonic::Response<proto::GetResponse>, Status> {
        use crate::traits::S3MultipartRegistry as _;
        let id = MultipartUploadId::new(request.into_inner().upload_id).map_err(into_status)?;
        let upload = self.registry.get(&id).await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::GetResponse {
            upload: upload.map(encode_upload),
        }))
    }

    async fn remove(
        &self,
        request: tonic::Request<proto::UploadRequest>,
    ) -> Result<tonic::Response<proto::Empty>, Status> {
        use crate::traits::S3MultipartRegistry as _;
        let id = MultipartUploadId::new(request.into_inner().upload_id).map_err(into_status)?;
        self.registry.remove(&id).await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::Empty {}))
    }
}

#[cfg(all(test, feature = "grpc-client"))]
mod tests {
    use super::*;

    #[test]
    fn malformed_error_details_remain_rpc_errors() -> Result<(), Box<dyn std::error::Error>> {
        for status in [
            Status::with_details(Code::AlreadyExists, "duplicate", vec![0xff].into()),
            Status::with_details(
                Code::AlreadyExists,
                "duplicate",
                proto::RegistryError {
                    kind: proto::registry_error::Kind::BlobAlreadyExists as i32,
                    blob_id: vec![0; 15],
                }
                .encode_to_vec()
                .into(),
            ),
            Status::with_details(
                Code::NotFound,
                "wrong code",
                proto::RegistryError {
                    kind: proto::registry_error::Kind::BlobAlreadyExists as i32,
                    blob_id: 1_u128.to_be_bytes().to_vec(),
                }
                .encode_to_vec()
                .into(),
            ),
        ] {
            if !matches!(from_status(status), Error::Rpc(_)) {
                return Err("malformed details became a typed duplicate".into());
            }
        }
        Ok(())
    }
}

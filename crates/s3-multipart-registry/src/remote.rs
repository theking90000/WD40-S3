//! Connected gRPC client. Failed operations are not replayed.
use tonic::{Status, transport::Endpoint};

use crate::{
    Cursor, Error, MultipartUpload, MultipartUploadId, PageResult, UploadRecord,
    grpc::{self, proto},
    traits::{S3MultipartRegistry, validate_listing},
};

/// Clones share an HTTP/2 channel without an application mutex.
#[derive(Clone)]
pub struct RemoteRegistry {
    client:
        proto::s3_multipart_registry_client::S3MultipartRegistryClient<tonic::transport::Channel>,
}

impl RemoteRegistry {
    /// Requires a Tokio runtime. TLS configuration belongs to the caller's endpoint.
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
            client: proto::s3_multipart_registry_client::S3MultipartRegistryClient::new(channel)
                .max_decoding_message_size(grpc::MAX_MESSAGE_SIZE)
                .max_encoding_message_size(grpc::MAX_MESSAGE_SIZE),
        })
    }
}

impl S3MultipartRegistry for RemoteRegistry {
    async fn create(&self, upload: MultipartUpload) -> Result<MultipartUploadId, Error> {
        let response = self
            .client
            .clone()
            .create(proto::CreateRequest {
                upload: Some(grpc::encode_upload(upload)),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        MultipartUploadId::new(response.upload_id)
            .map_err(|_| Error::Rpc(Status::data_loss("invalid upload ID")))
    }

    async fn get(&self, id: &MultipartUploadId) -> Result<Option<MultipartUpload>, Error> {
        let response = self
            .client
            .clone()
            .get(proto::UploadRequest {
                upload_id: id.as_str().to_owned(),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        response
            .upload
            .map(grpc::decode_upload)
            .transpose()
            .map_err(|error| Error::Rpc(Status::data_loss(error.message())))
    }

    async fn list(
        &self,
        bucket: &str,
        prefix: &str,
        cursor: Option<&Cursor>,
        limit: Option<usize>,
    ) -> Result<PageResult, Error> {
        let limit = validate_listing(bucket, prefix, cursor, limit)?;
        let response = self
            .client
            .clone()
            .list(proto::ListRequest {
                bucket: bucket.to_owned(),
                prefix: prefix.to_owned(),
                cursor: cursor.cloned().map(grpc::encode_cursor),
                limit: Some(u32::try_from(limit).map_err(|_| Error::InvalidLimit)?),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        let records = response
            .records
            .into_iter()
            .map(|record| {
                let upload_id = MultipartUploadId::new(record.upload_id)
                    .map_err(|_| Error::Rpc(Status::data_loss("invalid upload ID")))?;
                let upload = record
                    .upload
                    .ok_or_else(|| Error::Rpc(Status::data_loss("missing listed upload")))?;
                let upload = grpc::decode_upload(upload)
                    .map_err(|error| Error::Rpc(Status::data_loss(error.message())))?;
                Ok(UploadRecord { upload_id, upload })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let next_cursor = response
            .next_cursor
            .map(grpc::decode_cursor)
            .transpose()
            .map_err(|error| Error::Rpc(Status::data_loss(error.to_string())))?;
        validate_listing(bucket, prefix, next_cursor.as_ref(), Some(limit))
            .map_err(|error| Error::Rpc(Status::data_loss(error.to_string())))?;
        Ok(PageResult {
            records,
            next_cursor,
        })
    }

    async fn remove(&self, id: &MultipartUploadId) -> Result<(), Error> {
        self.client
            .clone()
            .remove(proto::UploadRequest {
                upload_id: id.as_str().to_owned(),
            })
            .await
            .map_err(grpc::from_status)?;
        Ok(())
    }
}

//! Connected gRPC client. Failed operations are not replayed.
use tonic::{Status, transport::Endpoint};

use crate::{
    Credential, Error, KeyId, KeySecret, PrincipalId,
    grpc::{self, proto},
    traits::S3CredentialRegistry,
};

/// Clones share an HTTP/2 channel without an application mutex.
#[derive(Clone)]
pub struct RemoteRegistry {
    client:
        proto::s3_credential_registry_client::S3CredentialRegistryClient<tonic::transport::Channel>,
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
            client: proto::s3_credential_registry_client::S3CredentialRegistryClient::new(channel)
                .max_decoding_message_size(grpc::MAX_MESSAGE_SIZE)
                .max_encoding_message_size(grpc::MAX_MESSAGE_SIZE),
        })
    }
}

impl S3CredentialRegistry for RemoteRegistry {
    async fn register_key(
        &self,
        id: KeyId,
        secret: KeySecret,
        principal: PrincipalId,
    ) -> Result<(), Error> {
        self.client
            .clone()
            .register_key(proto::RegisterKeyRequest {
                id: id.as_str().to_owned(),
                secret: secret.as_str().to_owned(),
                principal_id: principal.to_be_bytes().to_vec(),
            })
            .await
            .map_err(grpc::from_status)?;
        Ok(())
    }

    async fn get_key(&self, id: &KeyId) -> Result<Option<Credential>, Error> {
        let response = self
            .client
            .clone()
            .get_key(proto::KeyRequest {
                id: id.as_str().to_owned(),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        response
            .credential
            .map(grpc::decode_credential)
            .transpose()
            .map_err(|error| Error::Rpc(Status::data_loss(error.message())))
    }

    async fn revoke_key(&self, id: &KeyId) -> Result<(), Error> {
        self.client
            .clone()
            .revoke_key(proto::KeyRequest {
                id: id.as_str().to_owned(),
            })
            .await
            .map_err(grpc::from_status)?;
        Ok(())
    }
}

//! Protocol v1 and a service delegating to the injected registry enum.
use prost::Message;
use tonic::{Code, Status};

#[cfg(feature = "grpc-client")]
use crate::Credential;
use crate::{Error, KeyId, KeySecret, PrincipalId};

pub mod proto {
    tonic::include_proto!("wd40.s3_credentials_registry.v1");
}

pub const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

pub(crate) fn decode_principal(bytes: &[u8]) -> Result<PrincipalId, Status> {
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("principal ID must contain exactly 16 bytes"))?;
    Ok(u128::from_be_bytes(bytes))
}

#[cfg(feature = "grpc-client")]
pub(crate) fn decode_credential(value: proto::Credential) -> Result<Credential, Status> {
    Ok(Credential {
        principal_id: decode_principal(&value.principal_id)?,
        secret: KeySecret::new(value.secret)
            .map_err(|_| Status::invalid_argument("invalid key secret length"))?,
    })
}

#[cfg(feature = "grpc-server")]
pub(crate) fn into_status(error: Error) -> Status {
    match error {
        Error::KeyAlreadyExists(id) => Status::with_details(
            Code::AlreadyExists,
            "key already exists",
            proto::RegistryError {
                kind: proto::registry_error::Kind::KeyAlreadyExists as i32,
                key_id: id.as_str().to_owned(),
            }
            .encode_to_vec()
            .into(),
        ),
        Error::InvalidKeyId => Status::invalid_argument("invalid key ID length"),
        Error::InvalidKeySecret => Status::invalid_argument("invalid key secret length"),
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
        && details.kind == proto::registry_error::Kind::KeyAlreadyExists as i32
        && let Ok(id) = KeyId::new(details.key_id)
    {
        return Error::KeyAlreadyExists(id);
    }
    Error::Rpc(status)
}

#[cfg(feature = "grpc-server")]
#[derive(Clone)]
pub struct GrpcService {
    registry: crate::S3CredentialRegistry,
}

#[cfg(feature = "grpc-server")]
impl GrpcService {
    pub fn new(registry: crate::S3CredentialRegistry) -> Self {
        Self { registry }
    }

    pub fn into_server(
        self,
    ) -> proto::s3_credential_registry_server::S3CredentialRegistryServer<Self> {
        proto::s3_credential_registry_server::S3CredentialRegistryServer::new(self)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE)
    }
}

#[cfg(feature = "grpc-server")]
#[tonic::async_trait]
impl proto::s3_credential_registry_server::S3CredentialRegistry for GrpcService {
    async fn register_key(
        &self,
        request: tonic::Request<proto::RegisterKeyRequest>,
    ) -> Result<tonic::Response<proto::Empty>, Status> {
        use crate::traits::S3CredentialRegistry as _;
        let request = request.into_inner();
        let id = KeyId::new(request.id).map_err(into_status)?;
        let secret = KeySecret::new(request.secret).map_err(into_status)?;
        let principal = decode_principal(&request.principal_id)?;
        self.registry
            .register_key(id, secret, principal)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::Empty {}))
    }

    async fn get_key(
        &self,
        request: tonic::Request<proto::KeyRequest>,
    ) -> Result<tonic::Response<proto::GetKeyResponse>, Status> {
        use crate::traits::S3CredentialRegistry as _;
        let id = KeyId::new(request.into_inner().id).map_err(into_status)?;
        let credential = self.registry.get_key(&id).await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::GetKeyResponse {
            credential: credential.map(|value| proto::Credential {
                principal_id: value.principal_id.to_be_bytes().to_vec(),
                secret: value.secret.as_str().to_owned(),
            }),
        }))
    }

    async fn list_keys(
        &self,
        request: tonic::Request<proto::ListKeysRequest>,
    ) -> Result<tonic::Response<proto::ListKeysResponse>, Status> {
        use crate::traits::S3CredentialRegistry as _;
        let principal = decode_principal(&request.into_inner().principal_id)?;
        let keys = self
            .registry
            .list_keys(principal)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::ListKeysResponse {
            ids: keys.into_iter().map(|id| id.as_str().to_owned()).collect(),
        }))
    }

    async fn revoke_key(
        &self,
        request: tonic::Request<proto::KeyRequest>,
    ) -> Result<tonic::Response<proto::Empty>, Status> {
        use crate::traits::S3CredentialRegistry as _;
        let id = KeyId::new(request.into_inner().id).map_err(into_status)?;
        self.registry.revoke_key(&id).await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::Empty {}))
    }
}

#[cfg(all(test, feature = "grpc-client"))]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[test]
    fn malformed_credential_responses_and_error_details_are_rejected() -> TestResult {
        for credential in [
            proto::Credential {
                principal_id: vec![0; 15],
                secret: "s".repeat(40),
            },
            proto::Credential {
                principal_id: vec![0; 17],
                secret: "s".repeat(40),
            },
            proto::Credential {
                principal_id: 0_u128.to_be_bytes().to_vec(),
                secret: "short".to_owned(),
            },
        ] {
            if decode_credential(credential).is_ok() {
                return Err("malformed credential response accepted".into());
            }
        }
        let credential = decode_credential(proto::Credential {
            principal_id: u128::MAX.to_be_bytes().to_vec(),
            secret: "密".repeat(40),
        })?;
        if credential.principal_id != u128::MAX || credential.secret.as_str() != "密".repeat(40) {
            return Err("valid Unicode response changed".into());
        }
        for status in [
            Status::with_details(Code::AlreadyExists, "duplicate", vec![0xff].into()),
            Status::with_details(
                Code::AlreadyExists,
                "duplicate",
                proto::RegistryError {
                    kind: proto::registry_error::Kind::KeyAlreadyExists as i32,
                    key_id: "short".to_owned(),
                }
                .encode_to_vec()
                .into(),
            ),
            Status::with_details(
                Code::NotFound,
                "wrong status",
                proto::RegistryError {
                    kind: proto::registry_error::Kind::KeyAlreadyExists as i32,
                    key_id: "A".repeat(20),
                }
                .encode_to_vec()
                .into(),
            ),
        ] {
            if !matches!(from_status(status), Error::Rpc(_)) {
                return Err("malformed error details accepted as typed duplicate".into());
            }
        }
        Ok(())
    }
}

//! Persistent S3 credentials with optional gRPC transport.

#[cfg(feature = "cli")]
pub mod cli;
pub mod error;
#[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
pub mod grpc;
pub mod local;
#[cfg(feature = "grpc-client")]
pub mod remote;
pub mod traits;

pub use error::Error;
pub use error::Error as RegistryError;
#[cfg(feature = "grpc-server")]
pub use grpc::GrpcService;
pub use local::LocalRegistry;
#[cfg(feature = "grpc-client")]
pub use remote::RemoteRegistry;
pub use traits::{Credential, KeyId, KeySecret, PrincipalId};

#[derive(Clone)]
pub enum S3CredentialRegistry {
    Local(LocalRegistry),
    #[cfg(feature = "grpc-client")]
    Remote(RemoteRegistry),
}

impl traits::S3CredentialRegistry for S3CredentialRegistry {
    async fn register_key(
        &self,
        id: KeyId,
        secret: KeySecret,
        principal: PrincipalId,
    ) -> Result<(), Error> {
        match self {
            Self::Local(registry) => {
                traits::S3CredentialRegistry::register_key(registry, id, secret, principal).await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::S3CredentialRegistry::register_key(registry, id, secret, principal).await
            }
        }
    }

    async fn get_key(&self, id: &KeyId) -> Result<Option<Credential>, Error> {
        match self {
            Self::Local(registry) => traits::S3CredentialRegistry::get_key(registry, id).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::S3CredentialRegistry::get_key(registry, id).await,
        }
    }

    async fn list_keys(&self, principal: PrincipalId) -> Result<Vec<KeyId>, Error> {
        match self {
            Self::Local(registry) => {
                traits::S3CredentialRegistry::list_keys(registry, principal).await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::S3CredentialRegistry::list_keys(registry, principal).await
            }
        }
    }

    async fn revoke_key(&self, id: &KeyId) -> Result<(), Error> {
        match self {
            Self::Local(registry) => traits::S3CredentialRegistry::revoke_key(registry, id).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::S3CredentialRegistry::revoke_key(registry, id).await,
        }
    }
}

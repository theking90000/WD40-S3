//! S3S authentication backed by the credentials registry.

use s3_credentials_registry::{KeyId, S3CredentialRegistry, traits::S3CredentialRegistry as _};
use s3s::{S3Error, S3Result, auth::SecretKey};

/// Retrieves signing secrets from an owned credentials registry handle.
#[derive(Clone)]
pub struct RegistryAuth {
    registry: S3CredentialRegistry,
}

impl RegistryAuth {
    /// Retains the registry without performing I/O.
    pub fn new(registry: S3CredentialRegistry) -> Self {
        Self { registry }
    }
}

#[async_trait::async_trait]
impl s3s::auth::S3Auth for RegistryAuth {
    async fn get_secret_key(&self, access_key: &str) -> S3Result<SecretKey> {
        let id = access_key
            .parse::<KeyId>()
            .map_err(|_| s3s::s3_error!(InvalidAccessKeyId))?;
        let credential = self
            .registry
            .get_key(&id)
            .await
            .map_err(S3Error::internal_error)?
            .ok_or_else(|| s3s::s3_error!(InvalidAccessKeyId))?;
        Ok(SecretKey::from(credential.secret.as_str()))
    }
}

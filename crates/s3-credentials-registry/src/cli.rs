//! S3 credentials registry options composable with `#[command(flatten)]`.

use std::path::PathBuf;

use crate::{Error, LocalRegistry, S3CredentialRegistry};

/// Command-line configuration selecting a local database or remote registry.
#[derive(clap::Args)]
pub struct S3CredentialsRegistryConfig {
    /// Persistent S3 credentials registry database directory.
    #[arg(
        long,
        global = true,
        env = "WD40_S3_CREDENTIALS_REGISTRY_PATH",
        default_value = "./data/s3-credentials-registry"
    )]
    pub s3_credentials_registry_path: PathBuf,

    /// gRPC endpoint. When set, the local database is not opened.
    #[cfg(feature = "grpc-client")]
    #[arg(long, global = true, env = "WD40_S3_CREDENTIALS_REGISTRY_ENDPOINT")]
    pub s3_credentials_registry_endpoint: Option<String>,

    /// Additional PEM CA trusted for the remote HTTPS endpoint.
    #[cfg(all(feature = "grpc-client", feature = "grpc-tls"))]
    #[arg(
        long,
        global = true,
        env = "WD40_S3_CREDENTIALS_REGISTRY_CA",
        requires = "s3_credentials_registry_endpoint"
    )]
    pub s3_credentials_registry_ca: Option<PathBuf>,
}

impl S3CredentialsRegistryConfig {
    /// Opens the database or connects to the endpoint without opening sled.
    /// Remote connections require a Tokio runtime and failures are returned.
    pub async fn open(&self) -> Result<S3CredentialRegistry, Error> {
        #[cfg(feature = "grpc-client")]
        if let Some(uri) = &self.s3_credentials_registry_endpoint {
            let endpoint = tonic::transport::Endpoint::from_shared(uri.clone()).map_err(|_| {
                Error::Rpc(tonic::Status::invalid_argument(
                    "invalid registry endpoint URI",
                ))
            })?;
            #[cfg(feature = "grpc-tls")]
            let endpoint = {
                if endpoint.uri().scheme_str() == Some("https") {
                    let mut tls = tonic::transport::ClientTlsConfig::new().with_native_roots();
                    if let Some(path) = &self.s3_credentials_registry_ca {
                        let pem = std::fs::read(path).map_err(|error| {
                            Error::Rpc(tonic::Status::internal(format!(
                                "cannot read registry CA: {error}"
                            )))
                        })?;
                        tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(pem));
                    }
                    endpoint.tls_config(tls).map_err(Error::Transport)?
                } else if self.s3_credentials_registry_ca.is_some() {
                    return Err(Error::Rpc(tonic::Status::invalid_argument(
                        "registry CA requires HTTPS",
                    )));
                } else {
                    endpoint
                }
            };
            return Ok(S3CredentialRegistry::Remote(
                crate::RemoteRegistry::connect(endpoint).await?,
            ));
        }
        Ok(S3CredentialRegistry::Local(LocalRegistry::new(
            sled::open(&self.s3_credentials_registry_path)?,
        )?))
    }
}

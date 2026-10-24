//! Metrics registry options composable with `#[command(flatten)]`.

use std::{path::PathBuf, time::Duration};

use crate::{Config, Error, LocalRegistry, MetricsRegistry, RetentionPolicy};

/// Command-line configuration selecting a local database or remote registry.
#[derive(clap::Args)]
pub struct MetricsRegistryConfig {
    /// Persistent metrics registry database directory.
    #[arg(
        long,
        global = true,
        env = "WD40_METRICS_REGISTRY_PATH",
        default_value = "./data/metrics-registry"
    )]
    pub metrics_registry_path: PathBuf,

    /// Local history policy as resolution:retention, both in whole seconds. Repeat for each resolution.
    #[arg(
        long,
        global = true,
        env = "WD40_METRICS_REGISTRY_POLICY",
        value_name = "RESOLUTION:RETENTION",
        value_delimiter = ',',
        value_parser = parse_policy
    )]
    #[cfg_attr(
        feature = "grpc-client",
        arg(required_unless_present = "metrics_registry_endpoint")
    )]
    #[cfg_attr(not(feature = "grpc-client"), arg(required = true))]
    pub metrics_registry_policy: Vec<RetentionPolicy>,

    /// gRPC endpoint. When set, local storage and retention policies are not used.
    #[cfg(feature = "grpc-client")]
    #[arg(long, global = true, env = "WD40_METRICS_REGISTRY_ENDPOINT")]
    pub metrics_registry_endpoint: Option<String>,

    /// Additional PEM CA trusted for the remote HTTPS endpoint.
    #[cfg(all(feature = "grpc-client", feature = "grpc-tls"))]
    #[arg(
        long,
        global = true,
        env = "WD40_METRICS_REGISTRY_CA",
        requires = "metrics_registry_endpoint"
    )]
    pub metrics_registry_ca: Option<PathBuf>,
}

impl MetricsRegistryConfig {
    /// Opens RocksDB with the required merge operators, or connects to the endpoint.
    /// Remote connections require a Tokio runtime. Local storage opens on a blocking worker.
    pub async fn open(&self) -> Result<MetricsRegistry, Error> {
        #[cfg(feature = "grpc-client")]
        if let Some(uri) = &self.metrics_registry_endpoint {
            let endpoint = tonic::transport::Endpoint::from_shared(uri.clone()).map_err(|_| {
                Error::Rpc(tonic::Status::invalid_argument(
                    "invalid registry endpoint URI",
                ))
            })?;
            #[cfg(feature = "grpc-tls")]
            let endpoint = {
                if endpoint.uri().scheme_str() == Some("https") {
                    let mut tls = tonic::transport::ClientTlsConfig::new().with_native_roots();
                    if let Some(path) = &self.metrics_registry_ca {
                        let pem = std::fs::read(path).map_err(|error| {
                            Error::Rpc(tonic::Status::internal(format!(
                                "cannot read registry CA: {error}"
                            )))
                        })?;
                        tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(pem));
                    }
                    endpoint.tls_config(tls).map_err(Error::Transport)?
                } else if self.metrics_registry_ca.is_some() {
                    return Err(Error::Rpc(tonic::Status::invalid_argument(
                        "registry CA requires HTTPS",
                    )));
                } else {
                    endpoint
                }
            };
            return Ok(MetricsRegistry::Remote(
                crate::RemoteRegistry::connect(endpoint).await?,
            ));
        }

        let config = Config {
            policies: self.metrics_registry_policy.clone(),
        };
        config.validate()?;
        let path = self.metrics_registry_path.clone();
        blocking::unblock(move || {
            let mut options = rocksdb::Options::default();
            options.create_if_missing(true);
            options.create_missing_column_families(true);
            options.set_compression_type(rocksdb::DBCompressionType::None);
            let database = rocksdb::DB::open_cf_descriptors(
                &options,
                path,
                LocalRegistry::column_families(&options),
            )
            .map_err(|error| Error::Backend(Box::new(error)))?;
            LocalRegistry::new(database, config).map(MetricsRegistry::Local)
        })
        .await
    }
}

fn parse_policy(value: &str) -> Result<RetentionPolicy, String> {
    let (resolution, retention) = value
        .split_once(':')
        .ok_or_else(|| "expected resolution:retention in whole seconds".to_owned())?;
    let resolution: u64 = resolution
        .parse()
        .map_err(|_| "resolution must be an unsigned whole number of seconds".to_owned())?;
    let retention: u64 = retention
        .parse()
        .map_err(|_| "retention must be an unsigned whole number of seconds".to_owned())?;
    if resolution == 0 {
        return Err("resolution must be nonzero".to_owned());
    }
    if retention < resolution {
        return Err("retention must be at least as long as the resolution".to_owned());
    }
    Ok(RetentionPolicy {
        resolution: Duration::from_secs(resolution),
        retention: Duration::from_secs(retention),
    })
}

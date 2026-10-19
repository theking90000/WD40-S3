use std::{io, sync::Arc};

use axum::Router;
use hyper_util::service::TowerToHyperService;
use policy_registry::PolicyRegistry;
use portal_db::DatabaseConnection;
use s3_credentials_registry::S3CredentialRegistry;

use crate::api;
use crate::api::auth::{OidcConfig, oidc::Oidc};
#[cfg(feature = "embed-console")]
use crate::assets;
use crate::sync::SyncContext;

#[derive(Clone)]
pub struct Portal {
    router: Router,
    sync: SyncContext,
}

impl Portal {
    /// The caller supplies a migrated database and opened registry handles.
    /// It owns database shutdown; construction performs no registry operations.
    pub async fn new(
        config: OidcConfig,
        db: DatabaseConnection,
        credentials: S3CredentialRegistry,
        policies: PolicyRegistry,
    ) -> io::Result<Self> {
        let oidc = Arc::new(Oidc::new(config).await?);
        let sync = SyncContext::new(db, credentials, policies);
        let router = api::router(oidc, sync.clone());
        #[cfg(feature = "embed-console")]
        let router = router.merge(assets::router());
        Ok(Self { router, sync })
    }

    /// Clone this context to share serialization with explicitly invoked operations.
    pub fn sync_context(&self) -> &SyncContext {
        &self.sync
    }

    pub fn into_router(self) -> Router {
        self.router
    }

    pub fn into_service(self) -> TowerToHyperService<Router> {
        TowerToHyperService::new(self.router)
    }
}

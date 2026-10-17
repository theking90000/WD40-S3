use std::{io, sync::Arc};

use axum::Router;
use hyper_util::service::TowerToHyperService;

use crate::api;
use crate::api::auth::{OidcConfig, oidc::Oidc};
#[cfg(feature = "embed-console")]
use crate::assets;

#[derive(Clone)]
pub struct Portal {
    router: Router,
}

impl Portal {
    pub async fn new(config: OidcConfig) -> io::Result<Self> {
        let oidc = Arc::new(Oidc::new(config).await?);
        let router = api::router(oidc);
        #[cfg(feature = "embed-console")]
        let router = router.merge(assets::router());
        Ok(Self { router })
    }

    pub fn into_router(self) -> Router {
        self.router
    }

    pub fn into_service(self) -> TowerToHyperService<Router> {
        TowerToHyperService::new(self.router)
    }
}


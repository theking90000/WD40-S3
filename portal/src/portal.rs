use axum::Router;
use hyper_util::service::TowerToHyperService;

use crate::api;
#[cfg(feature = "embed-console")]
use crate::assets;

#[derive(Clone)]
pub struct Portal {
    router: Router,
}

impl Portal {
    pub fn new() -> Self {
        let router = api::router();
        #[cfg(feature = "embed-console")]
        let router = router.merge(assets::router());
        Self { router }
    }

    pub fn into_router(self) -> Router {
        self.router
    }

    pub fn into_service(self) -> TowerToHyperService<Router> {
        TowerToHyperService::new(self.router)
    }
}

impl Default for Portal {
    fn default() -> Self {
        Self::new()
    }
}

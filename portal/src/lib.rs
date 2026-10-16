//! Embeddable portal service. The caller owns the listener and runtime.

#[cfg(feature = "embed-console")]
mod assets;
#[cfg(feature = "cli")]
pub mod cli;

use axum::{Router, routing::get};
use hyper_util::service::TowerToHyperService;

#[derive(Clone)]
pub struct Portal {
    router: Router,
}

impl Portal {
    pub fn new() -> Self {
        let router = Router::new().route("/api/health", get(|| async { "ok" }));
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

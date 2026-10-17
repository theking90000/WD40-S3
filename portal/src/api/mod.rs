mod health;
pub(crate) mod auth;

use std::sync::Arc;
use axum::{Router, routing::get};

pub(super) fn router(oidc: Arc<auth::oidc::Oidc>) -> Router {
    Router::new()
        .route("/api/health", get(health::handle))
        .merge(auth::router(oidc))
}

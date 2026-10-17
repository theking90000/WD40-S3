pub(crate) mod auth;
mod health;

use axum::{Router, routing::get};
use std::sync::Arc;

pub(super) fn router(oidc: Arc<auth::oidc::Oidc>) -> Router {
    Router::new()
        .route("/api/health", get(health::handle))
        .merge(auth::router(oidc))
}

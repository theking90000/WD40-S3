pub(crate) mod auth;
mod health;
mod organizations;

use axum::{Extension, Router, routing::get};
use std::sync::Arc;

use crate::sync::SyncContext;

pub(super) fn router(
    oidc: Arc<auth::oidc::Oidc>,
    sync: SyncContext,
    metrics: metrics_registry::MetricsRegistry,
) -> Router {
    let organizations = organizations::router(sync.db.clone())
        .layer(Extension(metrics))
        .layer(Extension(sync.credentials.clone()))
        .layer(Extension(sync.policies.clone()))
        .layer(Extension(sync.clone()));

    Router::new()
        .route("/api/health", get(health::handle))
        .merge(auth::router(oidc.clone()))
        .merge(auth::protect(organizations, oidc))
        .with_state(sync.db)
}

pub(crate) mod auth;
mod health;
mod organizations;

use axum::{Router, routing::get};
use portal_db::DatabaseConnection;
use std::sync::Arc;

pub(super) fn router(oidc: Arc<auth::oidc::Oidc>, db: DatabaseConnection) -> Router {
    Router::new()
        .route("/api/health", get(health::handle))
        .merge(auth::router(oidc.clone()))
        .merge(auth::protect(organizations::router(db.clone()), oidc))
        .with_state(db)
}

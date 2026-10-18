pub(crate) mod auth;
mod health;
mod organizations;

use axum::{Extension, Router, routing::get};
use policy_registry::PolicyRegistry;
use portal_db::DatabaseConnection;
use s3_credentials_registry::S3CredentialRegistry;
use std::sync::Arc;

pub(super) fn router(
    oidc: Arc<auth::oidc::Oidc>,
    db: DatabaseConnection,
    credentials: S3CredentialRegistry,
    policies: PolicyRegistry,
) -> Router {
    let organizations = organizations::router(db.clone())
        .layer(Extension(credentials))
        .layer(Extension(policies));

    Router::new()
        .route("/api/health", get(health::handle))
        .merge(auth::router(oidc.clone()))
        .merge(auth::protect(organizations, oidc))
        .with_state(db)
}

mod health;

use axum::{Router, routing::get};

pub(super) fn router() -> Router {
    Router::new().route("/api/health", get(health::handle))
}

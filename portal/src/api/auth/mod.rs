mod callback;
mod cookies;
mod login;
mod logout;
mod me;
mod middleware;
pub(crate) mod oidc;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;

pub use oidc::{CurrentUser, OidcConfig};

pub(super) fn router(oidc: Arc<oidc::Oidc>) -> Router {
    let protected = Router::new().route("/api/me", get(me::handle)).route_layer(
        axum::middleware::from_fn_with_state(oidc.clone(), middleware::authenticate),
    );

    Router::new()
        .route("/api/login", post(login::handle))
        .route("/api/callback", post(callback::handle))
        .route("/api/logout", post(logout::handle))
        .merge(protected)
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(axum::middleware::from_fn(middleware::no_store))
        .with_state(oidc)
}

pub(super) struct AuthError {
    status: StatusCode,
    code: &'static str,
}

impl AuthError {
    pub(super) fn new(status: StatusCode, code: &'static str) -> Self {
        Self { status, code }
    }

    pub(super) fn invalid_request() -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request")
    }

    pub(super) fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthenticated")
    }

    pub(super) fn upstream() -> Self {
        Self::new(StatusCode::BAD_GATEWAY, "invalid_oidc_response")
    }

    pub(super) fn unavailable() -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "oidc_unavailable")
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Body {
            error: &'static str,
        }
        (self.status, Json(Body { error: self.code })).into_response()
    }
}

// Requiring a non-safelisted header prevents browser cross-origin form submissions.
// Do not enable credentialed cross-origin access around these routes.
pub(super) fn browser_request(headers: &HeaderMap) -> Result<(), AuthError> {
    if headers
        .get("x-wd40-portal")
        .and_then(|value| value.to_str().ok())
        != Some("1")
    {
        return Err(AuthError::new(StatusCode::FORBIDDEN, "csrf_rejected"));
    }
    if headers
        .get("sec-fetch-site")
        .is_some_and(|value| value != "same-origin" && value != "none")
    {
        return Err(AuthError::new(StatusCode::FORBIDDEN, "csrf_rejected"));
    }
    Ok(())
}

pub(super) fn no_cache(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
}

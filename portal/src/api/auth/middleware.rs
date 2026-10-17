use std::sync::Arc;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use openidconnect::AccessToken;

use super::{
    AuthError,
    cookies::{self, Cookies},
    oidc::Oidc,
};

pub(super) async fn authenticate(
    State(oidc): State<Arc<Oidc>>,
    mut request: Request,
    next: Next,
) -> Response {
    let policy = Cookies::from_headers(request.headers());
    let jar = CookieJar::from_headers(request.headers());
    let user = match policy.get(&jar, cookies::ACCESS_TOKEN) {
        Some(value) if !value.is_empty() => {
            oidc.current_user(AccessToken::new(value.to_owned())).await
        }
        _ => Err(AuthError::unauthorized()),
    };
    match user {
        Ok(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(error) => {
            if error.status == axum::http::StatusCode::UNAUTHORIZED {
                (policy.clear(jar, cookies::ACCESS_TOKEN), error).into_response()
            } else {
                error.into_response()
            }
        }
    }
}

pub(super) async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    super::no_cache(&mut response);
    response
}

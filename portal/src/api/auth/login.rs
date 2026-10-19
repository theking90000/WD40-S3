use std::{borrow::Cow, sync::Arc};

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::HeaderMap,
};
use axum_extra::extract::cookie::CookieJar;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use openidconnect::{
    CsrfToken, Nonce, PkceCodeChallenge, PkceCodeVerifier, Scope,
    core::CoreAuthenticationFlow,
};
use serde::{Deserialize, Serialize};

use super::{
    AuthError, browser_request,
    cookies::{self, Cookies},
    oidc::Oidc,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    redirect_uri: String,
}

#[derive(Serialize)]
pub(super) struct Response {
    url: String,
}

fn random_secret() -> Result<String, AuthError> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::unavailable())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(super) async fn handle(
    State(oidc): State<Arc<Oidc>>,
    headers: HeaderMap,
    jar: CookieJar,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<(CookieJar, Json<Response>), AuthError> {
    browser_request(&headers)?;
    let Json(request) =
        payload.map_err(|error| AuthError::new(error.status(), "invalid_request"))?;
    let redirect_uri = cookies::redirect_uri(request.redirect_uri)?;
    let state = CsrfToken::new(random_secret()?);
    let nonce = Nonce::new(random_secret()?);
    let verifier = PkceCodeVerifier::new(random_secret()?);
    // The fallible generator always yields 43 base64url characters, within PKCE's 43..=128 range.
    let challenge = PkceCodeChallenge::from_code_verifier_sha256(&verifier);
    let (url, state, nonce) = oidc
        .client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            || state,
            || nonce,
        )
        .set_redirect_uri(Cow::Borrowed(&redirect_uri))
        .set_pkce_challenge(challenge)
        .add_scope(Scope::new("profile".to_owned()))
        .add_scope(Scope::new("email".to_owned()))
        .url();
    let policy = Cookies::from_headers(&headers);
    let jar = policy.set(
        jar,
        cookies::STATE,
        state.secret().to_owned(),
        cookies::LOGIN_SECONDS,
    );
    let jar = policy.set(
        jar,
        cookies::NONCE,
        nonce.secret().to_owned(),
        cookies::LOGIN_SECONDS,
    );
    let jar = policy.set(
        jar,
        cookies::PKCE,
        verifier.secret().to_owned(),
        cookies::LOGIN_SECONDS,
    );
    Ok((
        jar,
        Json(Response {
            url: url.to_string(),
        }),
    ))
}

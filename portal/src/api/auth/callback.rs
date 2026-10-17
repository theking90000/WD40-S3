use std::{borrow::Cow, sync::Arc};

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;
use openidconnect::{
    AccessTokenHash, AuthorizationCode, Nonce, OAuth2TokenResponse, PkceCodeVerifier,
    RequestTokenError, TokenIntrospectionResponse, TokenResponse,
    core::{CoreErrorResponseType, CoreTokenType},
};
use serde::Deserialize;

use super::{AuthError, browser_request, cookies::{self, Cookies}, oidc::{Oidc, unix_seconds}};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    code: String,
    state: String,
    redirect_uri: String,
}

pub(super) async fn handle(
    State(oidc): State<Arc<Oidc>>,
    headers: HeaderMap,
    jar: CookieJar,
    payload: Result<Json<Request>, JsonRejection>,
) -> Response {
    let policy = Cookies::from_headers(&headers);
    let result = complete(&oidc, &headers, jar, policy, payload).await;
    // Includes malformed JSON, missing state, denied browser requests and provider failures.
    policy.finish_callback(result.into_response())
}

async fn complete(
    oidc: &Oidc,
    headers: &HeaderMap,
    jar: CookieJar,
    policy: Cookies,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<(CookieJar, StatusCode), AuthError> {
    browser_request(headers)?;
    let Json(request) = payload.map_err(|_| AuthError::invalid_request())?;
    policy.check_state(&jar, &request.state)?;
    if request.code.is_empty() {
        return Err(AuthError::invalid_request());
    }
    let nonce = Nonce::new(policy.get(&jar, cookies::NONCE)
        .ok_or_else(AuthError::unauthorized)?.to_owned());
    let verifier = policy.get(&jar, cookies::PKCE).ok_or_else(AuthError::unauthorized)?;
    if !(43..=128).contains(&verifier.len())
        || !verifier.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
    {
        return Err(AuthError::unauthorized());
    }
    let redirect_uri = cookies::redirect_uri(request.redirect_uri)?;
    let tokens = oidc.client.exchange_code(AuthorizationCode::new(request.code))
        .set_redirect_uri(Cow::Borrowed(&redirect_uri))
        .set_pkce_verifier(PkceCodeVerifier::new(verifier.to_owned()))
        .request_async(&oidc.http).await.map_err(|error| match error {
            RequestTokenError::Request(_) => AuthError::unavailable(),
            RequestTokenError::ServerResponse(response)
                if response.error() == &CoreErrorResponseType::InvalidGrant => {
                    AuthError::new(StatusCode::BAD_REQUEST, "invalid_code")
                }
            _ => AuthError::upstream(),
        })?;
    if tokens.token_type() != &CoreTokenType::Bearer {
        return Err(AuthError::upstream());
    }
    let id_token = tokens.id_token().ok_or_else(AuthError::upstream)?;
    let verifier = oidc.client.id_token_verifier();
    let claims = id_token.claims(&verifier, &nonce).map_err(|_| AuthError::unauthorized())?;
    if claims.authorized_party().is_some_and(|value| value != oidc.client.client_id()) {
        return Err(AuthError::unauthorized());
    }
    if let Some(expected) = claims.access_token_hash() {
        let algorithm = id_token.signing_alg().map_err(|_| AuthError::unauthorized())?;
        let key = id_token.signing_key(&verifier).map_err(|_| AuthError::unauthorized())?;
        let actual = AccessTokenHash::from_token(tokens.access_token(), algorithm, key)
            .map_err(|_| AuthError::upstream())?;
        if &actual != expected {
            return Err(AuthError::unauthorized());
        }
    }
    let info = oidc.introspect(tokens.access_token()).await?;
    if info.sub() != Some(claims.subject().as_str()) {
        return Err(AuthError::unauthorized());
    }
    let remaining = info.exp().ok_or_else(AuthError::upstream)?.timestamp()
        .checked_sub(unix_seconds()?).filter(|seconds| *seconds > 0)
        .ok_or_else(AuthError::unauthorized)?;
    let seconds = match tokens.expires_in() {
        Some(duration) => remaining.min(i64::try_from(duration.as_secs()).map_err(|_| AuthError::upstream())?),
        None => remaining,
    };
    if seconds <= 0 {
        return Err(AuthError::unauthorized());
    }
    let jar = policy.set_token(jar, tokens.access_token().secret().to_owned(), seconds)?;
    Ok((jar, StatusCode::NO_CONTENT))
}

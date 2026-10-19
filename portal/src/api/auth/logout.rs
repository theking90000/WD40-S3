use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::CookieJar;

use super::{
    AuthError, browser_request,
    cookies::{self, Cookies},
};

pub(super) async fn handle(headers: HeaderMap, jar: CookieJar) -> Result<Response, AuthError> {
    browser_request(&headers)?;
    let policy = Cookies::from_headers(&headers);
    let jar = policy.clear(jar, cookies::ACCESS_TOKEN);
    Ok(policy.finish_callback((jar, StatusCode::NO_CONTENT).into_response()))
}

use std::net::IpAddr;

use axum::{
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use openidconnect::url::{Host, Url};
use subtle::ConstantTimeEq;

use super::AuthError;

pub(super) const STATE: &str = "state";
pub(super) const NONCE: &str = "nonce";
pub(super) const PKCE: &str = "pkce";
pub(super) const ACCESS_TOKEN: &str = "access_token";
pub(super) const LOGIN_SECONDS: i64 = 300;

#[derive(Clone, Copy)]
pub(super) struct Cookies {
    local_http: bool,
}

fn loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain("localhost")) => true,
        Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
        Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
        _ => false,
    }
}

impl Cookies {
    pub(super) fn from_headers(headers: &HeaderMap) -> Self {
        let host = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| Url::parse(&format!("http://{value}")).ok());
        let origin = headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| Url::parse(value).ok());
        let local_http = host.as_ref().is_some_and(loopback)
            && origin
                .as_ref()
                .is_none_or(|url| url.scheme() == "http" && loopback(url));
        Self { local_http }
    }

    fn cookie(self, kind: &str, value: String, seconds: i64) -> Cookie<'static> {
        let name = if self.local_http {
            format!("wd40_oidc_{kind}")
        } else {
            format!("__Host-wd40_oidc_{kind}")
        };
        Cookie::build((name, value))
            .path("/")
            .http_only(true)
            .secure(!self.local_http)
            .same_site(SameSite::Lax)
            .max_age(time::Duration::seconds(seconds))
            .build()
    }

    pub(super) fn set(self, jar: CookieJar, kind: &str, value: String, seconds: i64) -> CookieJar {
        jar.add(self.cookie(kind, value, seconds))
    }

    pub(super) fn set_token(
        self,
        jar: CookieJar,
        token: String,
        seconds: i64,
    ) -> Result<CookieJar, AuthError> {
        let cookie = self.cookie(ACCESS_TOKEN, token, seconds);
        if cookie.encoded().to_string().len() > 4096 {
            return Err(AuthError::upstream());
        }
        Ok(jar.add(cookie))
    }

    pub(super) fn get<'a>(self, jar: &'a CookieJar, kind: &str) -> Option<&'a str> {
        // Production only accepts __Host- cookies, preventing sibling-domain cookie injection.
        jar.get(&format!("__Host-wd40_oidc_{kind}"))
            .or_else(|| {
                if self.local_http {
                    jar.get(&format!("wd40_oidc_{kind}"))
                } else {
                    None
                }
            })
            .map(|cookie| cookie.value())
    }

    pub(super) fn clear(self, mut jar: CookieJar, kind: &str) -> CookieJar {
        // Adding an expired cookie emits Set-Cookie even when the request had no cookie.
        for local_http in [false, true] {
            jar = jar.add(Self { local_http }.cookie(kind, String::new(), 0));
        }
        jar
    }

    pub(super) fn finish_callback(self, response: Response) -> Response {
        let mut jar = CookieJar::new();
        for kind in [STATE, NONCE, PKCE] {
            jar = self.clear(jar, kind);
        }
        (jar, response).into_response()
    }

    pub(super) fn check_state(self, jar: &CookieJar, supplied: &str) -> Result<(), AuthError> {
        let expected = self.get(jar, STATE).ok_or_else(AuthError::unauthorized)?;
        if supplied.is_empty() || !bool::from(expected.as_bytes().ct_eq(supplied.as_bytes())) {
            return Err(AuthError::unauthorized());
        }
        Ok(())
    }
}

pub(super) fn redirect_uri(value: String) -> Result<openidconnect::RedirectUrl, AuthError> {
    let uri = openidconnect::RedirectUrl::new(value).map_err(|_| AuthError::invalid_request())?;
    let url = uri.url();
    if url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !(url.scheme() == "https" || url.scheme() == "http" && loopback(url))
    {
        return Err(AuthError::invalid_request());
    }
    Ok(uri)
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderValue, StatusCode};

    use super::*;

    #[test]
    fn state_is_bound_to_cookie_and_callback_always_clears_pending_cookies() {
        let policy = Cookies::from_headers(&HeaderMap::new());
        let jar = policy.set(
            CookieJar::new(),
            STATE,
            "expected".to_owned(),
            LOGIN_SECONDS,
        );
        assert!(policy.check_state(&jar, "expected").is_ok());
        assert!(policy.check_state(&jar, "other").is_err());
        assert!(policy.check_state(&jar, "").is_err());
        assert!(policy.check_state(&CookieJar::new(), "expected").is_err());

        let injected = CookieJar::new().add(Cookie::new("wd40_oidc_state", "expected"));
        assert!(policy.check_state(&injected, "expected").is_err());

        for status in [
            StatusCode::NO_CONTENT,
            StatusCode::BAD_REQUEST,
            StatusCode::SERVICE_UNAVAILABLE,
        ] {
            let response = policy.finish_callback(status.into_response());
            let deletions: Vec<_> = response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .collect();
            assert_eq!(response.status(), status);
            assert_eq!(deletions.len(), 6);
            assert!(
                deletions
                    .iter()
                    .all(|value| value.contains("Max-Age=0") && value.contains("HttpOnly"))
            );
            for kind in [STATE, NONCE, PKCE] {
                assert!(
                    deletions
                        .iter()
                        .any(|value| value.starts_with(&format!("__Host-wd40_oidc_{kind}=")))
                );
            }
        }

        let mut headers = HeaderMap::new();
        assert!(super::super::browser_request(&headers).is_err());
        headers.insert("x-wd40-portal", HeaderValue::from_static("1"));
        assert!(super::super::browser_request(&headers).is_ok());
        headers.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert!(super::super::browser_request(&headers).is_err());
    }
}

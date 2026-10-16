use axum::{
    Router,
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};

#[derive(rust_embed::Embed)]
#[folder = "console/build/client/"]
struct ConsoleAssets;

pub(crate) fn router() -> Router {
    Router::new().fallback_service(get(file))
}

async fn file(uri: Uri) -> Response {
    let path = uri.path().trim_matches('/');
    if path == "api" || path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let index = if path.is_empty() {
        "index.html".to_owned()
    } else {
        format!("{path}/index.html")
    };
    let basename = env!("WD40_PORTAL_BASE_PATH").trim_matches('/');
    let file = [path, index.as_str()].into_iter().find_map(|path| {
        ConsoleAssets::get(&format!("{basename}/{path}"))
            .or_else(|| ConsoleAssets::get(path))
    });
    let Some(file) = file else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype()),
            (header::CACHE_CONTROL, cache_control),
        ],
        Body::from(file.data),
    )
        .into_response()
}

#[cfg(all(test, feature = "standalone"))]
#[tokio::test]
async fn serves_prerendered_pages_and_returns_404_for_missing_paths() {
    for path in ["/", "/status", "/status/"] {
        let response = file(Uri::from_static(path)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("text/html"),
        );
    }
    for path in ["/missing", "/api/missing", "/assets/missing.js"] {
        assert_eq!(file(Uri::from_static(path)).await.status(), StatusCode::NOT_FOUND);
    }
}

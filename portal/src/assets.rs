use axum::{
    Router,
    body::Body,
    extract::Path,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};

#[derive(vite_rs::Embed)]
#[root = "./console"]
#[output = "./build/client"]
#[dev_server_port = "21012"]
struct ConsoleAssets;

pub(crate) fn router() -> Router {
    Router::new()
        .route("/", get(|| async { asset("index.html") }))
        .route("/{*path}", get(file))
}

async fn file(Path(path): Path<String>, uri: Uri) -> Response {
    if embedded(&path).is_some() {
        return asset(&path);
    }
    if path == "api" || path.starts_with("api/") || path == "assets" || path.starts_with("assets/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let index = format!("{}/index.html", path.trim_end_matches('/'));
    if embedded(&index).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if path.ends_with('/') {
        return asset(&index);
    }

    // Relative redirects preserve an outer Axum mount prefix.
    let Some(segment) = uri.path().rsplit('/').next() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut location = format!("./{segment}/");
    if let Some(query) = uri.query() {
        location.push('?');
        location.push_str(query);
    }
    Redirect::permanent(&location).into_response()
}

fn embedded(path: &str) -> Option<vite_rs::ViteFile> {
    ConsoleAssets::get(path).or_else(|| {
        let basename = env!("WD40_PORTAL_BASE_PATH").trim_matches('/');
        if basename.is_empty() {
            return None;
        }
        ConsoleAssets::get(&format!("{basename}/{path}"))
    })
}

fn asset(path: &str) -> Response {
    let Some(file) = embedded(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Everything under assets/ is emitted by Vite with a content hash.
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, file.content_type),
            (header::CACHE_CONTROL, cache_control),
        ],
        Body::from(file.bytes),
    )
        .into_response()
}

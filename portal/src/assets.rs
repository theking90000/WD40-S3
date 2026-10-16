use axum::{
    Router,
    body::Body,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};

#[derive(rust_embed::Embed)]
#[folder = "$OUT_DIR/console/dist/"]
struct ConsoleAssets;

pub(crate) fn router() -> Router {
    Router::new()
        .route("/", get(|| async { asset("index.html") }))
        .route("/{*path}", get(file))
}

async fn file(Path(path): Path<String>) -> Response {
    asset(&path)
}

fn asset(path: &str) -> Response {
    let Some(file) = ConsoleAssets::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = mime_guess::from_path(path).first_or_octet_stream();
    // Everything under assets/ is emitted by esbuild with a content hash.
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, content_type.as_ref()),
            (header::CACHE_CONTROL, cache_control),
        ],
        Body::from(file.data),
    )
        .into_response()
}

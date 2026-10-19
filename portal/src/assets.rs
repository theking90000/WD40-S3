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

fn organization_page(path: &str) -> bool {
    let mut segments = path.split('/');
    let Some(id) = segments.next() else {
        return false;
    };
    if id.len() != 36 || portal_db::sea_orm::entity::prelude::Uuid::parse_str(id).is_err() {
        return false;
    }
    match segments.next() {
        None => true,
        Some("status" | "members" | "settings") => segments.next().is_none(),
        Some("buckets" | "credentials") => match segments.next() {
            None => true,
            Some("new") => segments.next().is_none(),
            Some(_) => false,
        },
        Some(_) => false,
    }
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
    let file = [path, index.as_str()]
        .into_iter()
        .find_map(|path| {
            ConsoleAssets::get(&format!("{basename}/{path}")).or_else(|| ConsoleAssets::get(path))
        })
        .or_else(|| {
            if organization_page(path) {
                ConsoleAssets::get(&format!("{basename}/__spa-fallback.html"))
                    .or_else(|| ConsoleAssets::get("__spa-fallback.html"))
                    .or_else(|| {
                        // With a basename, React Router writes static pages below
                        // that prefix and renames the root SPA document to index.html.
                        if basename.is_empty() {
                            None
                        } else {
                            ConsoleAssets::get("index.html")
                        }
                    })
            } else {
                None
            }
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
    for path in [
        "/",
        "/login",
        "/login/",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/buckets",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/buckets/",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/buckets/new",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/buckets/new/",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/credentials",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/credentials/",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/credentials/new",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/credentials/new/",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/members",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/members/",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/settings",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/settings/",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/status",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/status/",
    ] {
        let response = file(Uri::from_static(path)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/html"),
        );
    }
    for path in [
        "/missing",
        "/status",
        "/api/missing",
        "/assets/missing.js",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/missing",
        "/e2b219e2-f69d-41cd-83b2-e3e791133b24/status/missing.js",
    ] {
        assert_eq!(
            file(Uri::from_static(path)).await.status(),
            StatusCode::NOT_FOUND
        );
    }
}

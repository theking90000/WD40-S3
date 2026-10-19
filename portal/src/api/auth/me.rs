use axum::{Extension, Json};

use super::CurrentUser;

pub(super) async fn handle(Extension(user): Extension<CurrentUser>) -> Json<CurrentUser> {
    Json(user)
}

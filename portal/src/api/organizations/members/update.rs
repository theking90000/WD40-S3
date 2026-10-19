use axum::{
    Extension, Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, OrganizationMemberAccess, organization_member,
    sea_orm::{ActiveModelTrait, DbErr, EntityTrait, IntoActiveModel, Set},
};
use serde::Deserialize;

use crate::api::{auth::CurrentUser, organizations::user_email};

use super::{CurrentOrganization, Error, MemberResponse};

#[derive(Deserialize)]
pub(super) struct MemberPath {
    email: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    access: Access,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Access {
    Member,
    Owner,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    Extension(user): Extension<CurrentUser>,
    path: Result<Path<MemberPath>, PathRejection>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<Json<MemberResponse>, Error> {
    organization.require_owner()?;
    let Path(path) = path?;
    if path.email == user_email(&user)? {
        return Err(Error::new(StatusCode::FORBIDDEN, "self_update_forbidden"));
    }
    let Json(request) = payload?;
    let member = organization_member::Entity::find_by_id((organization.id, path.email))
        .one(&db)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "member_not_found"))?;
    let mut member = member.into_active_model();
    member.access = Set(match request.access {
        Access::Member => OrganizationMemberAccess::Member,
        Access::Owner => OrganizationMemberAccess::Owner,
    });
    let member = member.update(&db).await.map_err(|error| match error {
        DbErr::RecordNotUpdated | DbErr::RecordNotFound(_) => {
            Error::new(StatusCode::NOT_FOUND, "member_not_found")
        }
        error => Error::from(error),
    })?;

    Ok(Json(MemberResponse::from(member)))
}

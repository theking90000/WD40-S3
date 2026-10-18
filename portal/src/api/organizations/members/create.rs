use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use portal_db::{
    DatabaseConnection, OrganizationMemberAccess, organization_member,
    sea_orm::{ActiveModelTrait, Set, SqlErr},
};
use serde::Deserialize;

use super::{CurrentOrganization, Error, MemberResponse};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    email: String,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(organization): Extension<CurrentOrganization>,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<(StatusCode, Json<MemberResponse>), Error> {
    organization.require_owner()?;
    let Json(request) = payload?;
    let email = request.email.trim();
    let has_parts = email.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty() && !domain.is_empty() && !domain.contains('@')
    });
    if !has_parts
        || email.len() > 254
        || email
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(Error::new(StatusCode::BAD_REQUEST, "invalid_email"));
    }

    let member = organization_member::ActiveModel {
        organization_id: Set(organization.id),
        email: Set(email.to_owned()),
        access: Set(OrganizationMemberAccess::Member),
        ..Default::default()
    }
    .insert(&db)
    .await
    .map_err(|error| match error.sql_err() {
        Some(SqlErr::UniqueConstraintViolation(_)) => {
            Error::new(StatusCode::CONFLICT, "member_already_exists")
        }
        _ => Error::from(error),
    })?;

    Ok((StatusCode::CREATED, Json(MemberResponse::from(member))))
}

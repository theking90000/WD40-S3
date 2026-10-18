use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};
use portal_db::{
    DatabaseConnection, OrganizationMemberAccess, organization, organization_member,
    sea_orm::{ActiveModelTrait, Set, TransactionTrait},
};
use serde::Deserialize;

use crate::api::auth::{CurrentUser, browser_request};

use super::{Error, OrganizationResponse, verified_email};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    name: String,
}

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(user): Extension<CurrentUser>,
    headers: HeaderMap,
    payload: Result<Json<Request>, JsonRejection>,
) -> Result<(StatusCode, Json<OrganizationResponse>), Error> {
    browser_request(&headers).map_err(|_| Error::new(StatusCode::FORBIDDEN, "csrf_rejected"))?;
    let email = verified_email(&user)?;
    let Json(request) = payload?;
    let name = request.name.trim();
    if !(1..=128).contains(&name.chars().count()) {
        return Err(Error::new(StatusCode::BAD_REQUEST, "invalid_name"));
    }

    let transaction = db.begin().await?;
    let organization = organization::ActiveModel {
        name: Set(name.to_owned()),
        ..Default::default()
    }
    .insert(&transaction)
    .await?;
    organization_member::ActiveModel {
        organization_id: Set(organization.id),
        email: Set(email.to_owned()),
        access: Set(OrganizationMemberAccess::Owner),
        ..Default::default()
    }
    .insert(&transaction)
    .await?;
    transaction.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(OrganizationResponse::new(
            organization.id,
            organization.name,
            organization.created_at,
            OrganizationMemberAccess::Owner,
        )),
    ))
}

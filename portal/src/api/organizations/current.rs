use axum::{
    Extension,
    extract::{Path, Request, State, rejection::PathRejection},
    http::{Method, StatusCode},
    middleware::Next,
    response::Response,
};
use portal_db::{
    DatabaseConnection, OrganizationMemberAccess, organization_member,
    sea_orm::{EntityTrait, entity::prelude::Uuid},
};
use serde::Deserialize;

use crate::api::auth::{CurrentUser, browser_request};

use super::{Error, verified_email};

/// The authenticated user's membership in the organization selected by the URL.
#[derive(Clone, Copy)]
pub(super) struct CurrentOrganization {
    pub id: Uuid,
    pub access: OrganizationMemberAccess,
}

impl CurrentOrganization {
    pub(super) fn require_owner(&self) -> Result<(), Error> {
        match self.access {
            OrganizationMemberAccess::Owner => Ok(()),
            OrganizationMemberAccess::Member => {
                Err(Error::new(StatusCode::FORBIDDEN, "owner_required"))
            }
        }
    }
}

#[derive(Deserialize)]
pub(super) struct OrganizationPath {
    organization_id: String,
}

pub(super) async fn resolve(
    State(db): State<DatabaseConnection>,
    Extension(user): Extension<CurrentUser>,
    path: Result<Path<OrganizationPath>, PathRejection>,
    mut request: Request,
    next: Next,
) -> Result<Response, Error> {
    if request.method() == Method::POST
        || request.method() == Method::DELETE
        || request.method() == Method::PATCH
    {
        browser_request(request.headers())
            .map_err(|_| Error::new(StatusCode::FORBIDDEN, "csrf_rejected"))?;
    }
    let Path(path) = path?;
    let id = Uuid::parse_str(&path.organization_id)
        .map_err(|_| Error::new(StatusCode::BAD_REQUEST, "invalid_organization_id"))?;
    let email = verified_email(&user)?;
    let member = organization_member::Entity::find_by_id((id, email.to_owned()))
        .one(&db)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "organization_not_found"))?;

    request.extensions_mut().insert(CurrentOrganization {
        id,
        access: member.access,
    });
    Ok(next.run(request).await)
}

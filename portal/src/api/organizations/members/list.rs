use axum::{
    Extension, Json,
    extract::{Path, State, rejection::PathRejection},
};
use portal_db::{
    DatabaseConnection, organization_member,
    sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder},
};

use crate::api::auth::CurrentUser;

use super::{Error, MemberResponse, organization_id, require_member};

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(user): Extension<CurrentUser>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Json<Vec<MemberResponse>>, Error> {
    let Path(id) = path?;
    let id = organization_id(&id)?;
    require_member(&db, id, &user).await?;
    let members = organization_member::Entity::find()
        .filter(organization_member::Column::OrganizationId.eq(id))
        .order_by_asc(organization_member::Column::Email)
        .all(&db)
        .await?;

    Ok(Json(
        members.into_iter().map(MemberResponse::from).collect(),
    ))
}

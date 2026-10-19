use axum::{Extension, extract::State, http::StatusCode};
use portal_db::{
    DatabaseConnection, bucket, credential, organization,
    sea_orm::{
        ColumnTrait, ConnectionTrait, DbBackend, EntityTrait, PaginatorTrait, QueryFilter,
        QuerySelect, TransactionTrait, sea_query::Expr,
    },
};

use super::{Error, current::CurrentOrganization};

pub(super) async fn handle(
    State(db): State<DatabaseConnection>,
    Extension(current): Extension<CurrentOrganization>,
) -> Result<StatusCode, Error> {
    current.require_owner()?;
    let transaction = db.begin().await?;
    let mut query = organization::Entity::find_by_id(current.id);
    if transaction.get_database_backend() == DbBackend::Postgres {
        // Foreign-key inserts take a key-share lock on this row and must wait.
        query = query.lock_exclusive();
    } else {
        // SQLite has no row locks. Acquire its write lock before reading children.
        organization::Entity::update_many()
            .col_expr(
                organization::Column::Name,
                Expr::col(organization::Column::Name).into(),
            )
            .filter(organization::Column::Id.eq(current.id))
            .exec(&transaction)
            .await?;
    }
    query
        .one(&transaction)
        .await?
        .ok_or_else(|| Error::new(StatusCode::NOT_FOUND, "organization_not_found"))?;

    let buckets = bucket::Entity::find()
        .filter(bucket::Column::OrganizationId.eq(current.id))
        .count(&transaction)
        .await?;
    let credentials = credential::Entity::find()
        .filter(credential::Column::OrganizationId.eq(current.id))
        .count(&transaction)
        .await?;
    if buckets != 0 || credentials != 0 {
        return Err(Error::new(StatusCode::CONFLICT, "organization_not_empty"));
    }

    organization::Entity::delete_by_id(current.id)
        .exec(&transaction)
        .await?;
    transaction.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

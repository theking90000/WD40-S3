use axum::{
    Extension, Json,
    extract::{State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};
use portal_db::{
    DatabaseConnection, DbErr, OrganizationMemberAccess, organization, organization_member,
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
    .await
    .map_err(insert_error)?;
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

fn insert_error(error: DbErr) -> Error {
    #[cfg(any(feature = "sqlite", feature = "postgres"))]
    if let DbErr::Exec(portal_db::sea_orm::RuntimeErr::SqlxError(source))
    | DbErr::Query(portal_db::sea_orm::RuntimeErr::SqlxError(source)) = &error
        && let Some(database) = source.as_database_error()
        && database.is_unique_violation()
        && (database.constraint() == Some("idx-organizations-name")
            || database.message() == "UNIQUE constraint failed: organizations.name")
    {
        return Error::new(StatusCode::CONFLICT, "organization_name_taken");
    }
    Error::from(error)
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use portal_db::{
        Database, Migrator, MigratorTrait,
        sea_orm::{EntityTrait, PaginatorTrait},
    };

    #[tokio::test]
    async fn unique_name_migration_preserves_existing_data() -> Result<(), DbErr> {
        let db = Database::connect("sqlite::memory:").await?;
        Migrator::up(&db, Some(3)).await?;
        for _ in 0..2 {
            organization::ActiveModel {
                name: Set("Acme".to_owned()),
                ..Default::default()
            }
            .insert(&db)
            .await?;
        }
        if Migrator::up(&db, None).await.is_ok() {
            return Err(DbErr::Custom(
                "migration accepted duplicate names".to_owned(),
            ));
        }
        if organization::Entity::find().count(&db).await? != 2 {
            return Err(DbErr::Custom("migration changed existing rows".to_owned()));
        }

        let existing = organization::Entity::find()
            .one(&db)
            .await?
            .ok_or_else(|| DbErr::Custom("missing existing organization".to_owned()))?;
        organization::Entity::delete_by_id(existing.id)
            .exec(&db)
            .await?;
        Migrator::up(&db, None).await?;
        Migrator::down(&db, Some(1)).await?;
        organization::ActiveModel {
            name: Set("Acme".to_owned()),
            ..Default::default()
        }
        .insert(&db)
        .await?;
        db.close().await
    }

    #[tokio::test]
    async fn creation_rejects_duplicate_names_without_adding_members() -> Result<(), DbErr> {
        let db = Database::connect("sqlite::memory:").await?;
        Migrator::up(&db, None).await?;
        let user = CurrentUser {
            issuer: "https://identity.example".to_owned(),
            subject: "alice".to_owned(),
            name: None,
            email: Some("alice@example.org".to_owned()),
            email_verified: Some(true),
            roles: Vec::new(),
            groups: Vec::new(),
        };
        let mut headers = HeaderMap::new();
        headers.insert("x-wd40-portal", axum::http::HeaderValue::from_static("1"));
        for (name, expected) in [
            (" Acme ", StatusCode::CREATED),
            ("Acme", StatusCode::CONFLICT),
            ("acme", StatusCode::CREATED),
        ] {
            let result = handle(
                State(db.clone()),
                Extension(user.clone()),
                headers.clone(),
                Ok(Json(Request {
                    name: name.to_owned(),
                })),
            )
            .await;
            let status = match result {
                Ok((status, Json(organization))) => {
                    if organization.name != name.trim() {
                        return Err(DbErr::Custom(
                            "organization name was not trimmed".to_owned(),
                        ));
                    }
                    status
                }
                Err(error) => {
                    if error.code != "organization_name_taken" {
                        return Err(DbErr::Custom(format!(
                            "unexpected API error: {}",
                            error.code
                        )));
                    }
                    error.status
                }
            };
            if status != expected {
                return Err(DbErr::Custom(format!(
                    "expected {expected}, received {status}"
                )));
            }
        }
        if organization::Entity::find().count(&db).await? != 2
            || organization_member::Entity::find().count(&db).await? != 2
        {
            return Err(DbErr::Custom(
                "unexpected organization or membership count".to_owned(),
            ));
        }

        let existing = organization::Entity::find()
            .one(&db)
            .await?
            .ok_or_else(|| DbErr::Custom("missing organization".to_owned()))?;
        let duplicate_id = organization::ActiveModel {
            id: Set(existing.id),
            name: Set("Another organization".to_owned()),
            ..Default::default()
        }
        .insert(&db)
        .await;
        match duplicate_id {
            Err(error) => {
                if insert_error(error).status != StatusCode::INTERNAL_SERVER_ERROR {
                    return Err(DbErr::Custom(
                        "UUID conflict was not an internal error".to_owned(),
                    ));
                }
            }
            Ok(_) => return Err(DbErr::Custom("duplicate UUID was accepted".to_owned())),
        }
        db.close().await
    }
}

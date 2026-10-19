use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, entity::prelude::Uuid};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        manager
            .alter_table(
                Table::alter()
                    .table(Credentials::Table)
                    .add_column(
                        ColumnDef::new(Credentials::Permissions)
                            .small_integer()
                            .not_null()
                            .default(15_i16)
                            .check(Expr::col(Credentials::Permissions).between(0_i16, 15_i16)),
                    )
                    .to_owned(),
            )
            .await?;
        manager.create_table(
            Table::create()
                .table(Alias::new("legacy_bucket_policies"))
                .col(ColumnDef::new(Alias::new("policy_id")).uuid().not_null().primary_key())
                .to_owned(),
        ).await?;
        let legacy_policies = Query::insert()
            .into_table(Alias::new("legacy_bucket_policies"))
            .columns([Alias::new("policy_id")])
            .select_from(
                Query::select()
                    .column(Alias::new("private_policy_id"))
                    .from(Alias::new("bucket_sync"))
                    .to_owned(),
            )
            .map_err(|error| DbErr::Custom(error.to_string()))?
            .to_owned();
        db.execute(&legacy_policies).await?;

        create_sync_table(manager, "bucket_sync_v2", false).await?;
        let bucket_columns = [
            Alias::new("bucket_name"),
            Alias::new("public_policy_id"),
            Alias::new("generation"),
            Alias::new("applied_generation"),
            Alias::new("last_error"),
            Alias::new("delete_requested"),
        ];
        let buckets = Query::insert()
            .into_table(Alias::new("bucket_sync_v2"))
            .columns(bucket_columns.clone())
            .select_from(
                Query::select()
                    .columns(bucket_columns)
                    .from(Alias::new("bucket_sync"))
                    .to_owned(),
            )
            .map_err(|error| DbErr::Custom(error.to_string()))?
            .to_owned();
        db.execute(&buckets).await?;
        replace_table(manager, "bucket_sync").await?;

        create_sync_table(manager, "credential_sync_v2", true).await?;
        let rows = db.query_all(
            &Query::select()
                .columns([
                    Alias::new("key_id"), Alias::new("principal_id"),
                    Alias::new("generation"), Alias::new("delete_requested"),
                ])
                .from(Alias::new("credential_sync"))
                .to_owned(),
        ).await?;
        for row in rows {
            let key_id: String = row.try_get("", "key_id")?;
            let principal_id: Uuid = row.try_get("", "principal_id")?;
            let generation: i64 = row.try_get("", "generation")?;
            let delete_requested: bool = row.try_get("", "delete_requested")?;
            let policy_id = crate::entities::new_sync_id()?;
            let insert = Query::insert()
                .into_table(Alias::new("credential_sync_v2"))
                .columns([
                    Alias::new("key_id"), Alias::new("principal_id"), Alias::new("policy_id"),
                    Alias::new("generation"), Alias::new("applied_generation"),
                    Alias::new("last_error"), Alias::new("delete_requested"),
                ])
                .values([
                    key_id.into(), principal_id.into(), policy_id.into(), generation.into(),
                    0_i64.into(), "policy_migration_pending".into(), delete_requested.into(),
                ])
                .map_err(|error| DbErr::Custom(error.to_string()))?
                .to_owned();
            db.execute(&insert).await?;
        }
        replace_table(manager, "credential_sync").await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // SQL cannot restore policies and bindings removed from WD40 registries.
        Err(DbErr::Custom(
            "Credential policy migration requires a coordinated SQL and WD40 registry restore to roll back".to_owned(),
        ))
    }
}

async fn create_sync_table(manager: &SchemaManager<'_>, name: &str, credential: bool) -> Result<(), DbErr> {
    let mut table = Table::create();
    table.table(Alias::new(name));
    if credential {
        table
            .col(ColumnDef::new(Alias::new("key_id")).text().not_null().primary_key())
            .col(ColumnDef::new(Alias::new("principal_id")).uuid().not_null().unique_key())
            .col(ColumnDef::new(Alias::new("policy_id")).uuid().not_null().unique_key());
    } else {
        table
            .col(ColumnDef::new(Alias::new("bucket_name")).text().not_null().primary_key())
            .col(ColumnDef::new(Alias::new("public_policy_id")).uuid().not_null().unique_key());
    }
    table
        .col(ColumnDef::new(Alias::new("generation")).big_integer().not_null().default(1_i64))
        .col(ColumnDef::new(Alias::new("applied_generation")).big_integer().not_null().default(0_i64))
        .col(ColumnDef::new(Alias::new("last_error")).text())
        .col(ColumnDef::new(Alias::new("delete_requested")).boolean().not_null().default(false))
        .check(Expr::col(Alias::new("generation")).gte(1_i64))
        .check(Expr::col(Alias::new("applied_generation")).gte(0_i64))
        .check(Expr::col(Alias::new("applied_generation")).lte(Expr::col(Alias::new("generation"))));
    manager.create_table(table.to_owned()).await
}

async fn replace_table(manager: &SchemaManager<'_>, name: &str) -> Result<(), DbErr> {
    manager.drop_table(Table::drop().table(Alias::new(name)).to_owned()).await?;
    manager.rename_table(
        Table::rename().table(Alias::new(format!("{name}_v2")), Alias::new(name)).to_owned(),
    ).await
}

#[derive(DeriveIden)]
enum Credentials {
    Table,
    Permissions,
}

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Buckets::Table)
                    .col(
                        ColumnDef::new(Buckets::Name)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Buckets::OrganizationId).uuid().not_null())
                    .col(
                        ColumnDef::new(Buckets::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(Buckets::Access).text().not_null())
                    .check(
                        Expr::col(Buckets::Access).is_in(["PRIVATE", "PUBLIC", "PUBLIC_LIST"]),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-buckets-organizations")
                            .from(Buckets::Table, Buckets::OrganizationId)
                            .to(Organizations::Table, Organizations::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-buckets-organization_id")
                    .table(Buckets::Table)
                    .col(Buckets::OrganizationId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Buckets::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Buckets {
    Table,
    Name,
    OrganizationId,
    CreatedAt,
    Access,
}

#[derive(DeriveIden)]
enum Organizations {
    Table,
    Id,
}

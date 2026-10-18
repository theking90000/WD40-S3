use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Credentials::Table)
                    .col(
                        ColumnDef::new(Credentials::KeyId)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Credentials::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Credentials::AccessType).text().not_null())
                    .col(
                        ColumnDef::new(Credentials::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .check(Expr::col(Credentials::AccessType).is_in(["ALL", "RESTRICTED"]))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-credentials-organizations")
                            .from(Credentials::Table, Credentials::OrganizationId)
                            .to(Organizations::Table, Organizations::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx-credentials-organization_id")
                    .table(Credentials::Table)
                    .col(Credentials::OrganizationId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Credentials::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Credentials {
    Table,
    KeyId,
    OrganizationId,
    AccessType,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Organizations {
    Table,
    Id,
}

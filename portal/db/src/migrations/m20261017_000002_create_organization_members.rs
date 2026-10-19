use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(OrganizationMembers::Table)
                    .col(
                        ColumnDef::new(OrganizationMembers::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(OrganizationMembers::Email).text().not_null())
                    .col(
                        ColumnDef::new(OrganizationMembers::Access)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(OrganizationMembers::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(
                        Index::create()
                            .col(OrganizationMembers::OrganizationId)
                            .col(OrganizationMembers::Email),
                    )
                    .check(Expr::col(OrganizationMembers::Access).is_in(["MEMBER", "OWNER"]))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-organization_members-organizations")
                            .from(
                                OrganizationMembers::Table,
                                OrganizationMembers::OrganizationId,
                            )
                            .to(Organizations::Table, Organizations::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(OrganizationMembers::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum OrganizationMembers {
    Table,
    OrganizationId,
    Email,
    Access,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Organizations {
    Table,
    Id,
}

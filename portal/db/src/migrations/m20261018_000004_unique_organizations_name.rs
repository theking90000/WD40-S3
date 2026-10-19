use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx-organizations-name")
                    .table(Organizations::Table)
                    .col(Organizations::Name)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx-organizations-name")
                    .table(Organizations::Table)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Organizations {
    Table,
    Name,
}

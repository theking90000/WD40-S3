use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("uidx-credentials-organization_id-key_id")
                    .table(Credentials::Table)
                    .col(Credentials::OrganizationId)
                    .col(Credentials::KeyId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("uidx-buckets-organization_id-name")
                    .table(Buckets::Table)
                    .col(Buckets::OrganizationId)
                    .col(Buckets::Name)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_table(
                Table::create()
                    .table(CredentialBuckets::Table)
                    .col(
                        ColumnDef::new(CredentialBuckets::OrganizationId)
                            .uuid()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CredentialBuckets::KeyId).text().not_null())
                    .col(
                        ColumnDef::new(CredentialBuckets::BucketName)
                            .text()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(CredentialBuckets::KeyId)
                            .col(CredentialBuckets::BucketName),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-credential_buckets-credentials")
                            .from(
                                CredentialBuckets::Table,
                                (CredentialBuckets::OrganizationId, CredentialBuckets::KeyId),
                            )
                            .to(
                                Credentials::Table,
                                (Credentials::OrganizationId, Credentials::KeyId),
                            )
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-credential_buckets-buckets")
                            .from(
                                CredentialBuckets::Table,
                                (
                                    CredentialBuckets::OrganizationId,
                                    CredentialBuckets::BucketName,
                                ),
                            )
                            .to(Buckets::Table, (Buckets::OrganizationId, Buckets::Name))
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx-credential_buckets-organization_id-bucket_name")
                    .table(CredentialBuckets::Table)
                    .col(CredentialBuckets::OrganizationId)
                    .col(CredentialBuckets::BucketName)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CredentialBuckets::Table).to_owned())
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("uidx-buckets-organization_id-name")
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("uidx-credentials-organization_id-key_id")
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum CredentialBuckets {
    Table,
    OrganizationId,
    KeyId,
    BucketName,
}

#[derive(DeriveIden)]
enum Credentials {
    Table,
    OrganizationId,
    KeyId,
}

#[derive(DeriveIden)]
enum Buckets {
    Table,
    OrganizationId,
    Name,
}

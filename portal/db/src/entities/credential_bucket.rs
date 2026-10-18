use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "credential_buckets")]
pub struct Model {
    pub organization_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub key_id: String,
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub bucket_name: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::credential::Entity",
        from = "(Column::OrganizationId, Column::KeyId)",
        to = "(super::credential::Column::OrganizationId, super::credential::Column::KeyId)",
        on_delete = "Cascade"
    )]
    Credential,
    #[sea_orm(
        belongs_to = "super::bucket::Entity",
        from = "(Column::OrganizationId, Column::BucketName)",
        to = "(super::bucket::Column::OrganizationId, super::bucket::Column::Name)",
        on_delete = "Cascade"
    )]
    Bucket,
}

impl Related<super::credential::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Credential.def()
    }
}

impl Related<super::bucket::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Bucket.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

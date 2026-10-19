use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "buckets")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub name: String,
    pub organization_id: Uuid,
    pub created_at: DateTimeWithTimeZone,
    pub access: BucketAccess,
}

/// S3 access policy metadata. Enforcement belongs to the S3 access layer.
#[derive(Copy, Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Text")]
pub enum BucketAccess {
    /// Authentication is required for every operation.
    #[sea_orm(string_value = "PRIVATE")]
    Private,
    /// GET and HEAD object require no key. All other operations require authentication.
    #[sea_orm(string_value = "PUBLIC")]
    Public,
    /// GET and HEAD object, and LIST objects, require no key.
    /// All other operations require authentication.
    #[sea_orm(string_value = "PUBLIC_LIST")]
    PublicList,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::credential_bucket::Entity")]
    CredentialBucket,
    #[sea_orm(
        belongs_to = "super::organization::Entity",
        from = "Column::OrganizationId",
        to = "super::organization::Column::Id",
        on_delete = "Cascade"
    )]
    Organization,
}

impl Related<super::credential_bucket::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CredentialBucket.def()
    }
}

impl Related<super::organization::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Organization.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "credentials")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub key_id: String,
    pub organization_id: Uuid,
    pub access_type: CredentialAccessType,
    pub created_at: DateTimeWithTimeZone,
}

/// Organization bucket access metadata. Enforcement belongs to the S3 access layer.
#[derive(Copy, Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Text")]
pub enum CredentialAccessType {
    /// Access to every bucket of the organization, including future buckets.
    #[sea_orm(string_value = "ALL")]
    All,
    /// Access only to explicitly selected buckets. Without associations, grants no access.
    #[sea_orm(string_value = "RESTRICTED")]
    Restricted,
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

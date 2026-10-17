use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "organization_members")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub organization_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub email: String,
    pub access: OrganizationMemberAccess,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Text")]
pub enum OrganizationMemberAccess {
    #[sea_orm(string_value = "MEMBER")]
    Member,
    #[sea_orm(string_value = "OWNER")]
    Owner,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::organization::Entity",
        from = "Column::OrganizationId",
        to = "super::organization::Column::Id",
        on_delete = "Cascade"
    )]
    Organization,
}

impl Related<super::organization::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Organization.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

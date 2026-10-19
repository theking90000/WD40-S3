use sea_orm::entity::prelude::*;

/// Old shared private policies retained until explicit WD40 cleanup succeeds.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "legacy_bucket_policies")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub policy_id: Uuid,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

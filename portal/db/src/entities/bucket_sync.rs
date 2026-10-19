use sea_orm::{Set, entity::prelude::*};

/// Stable WD40 public policy identifier and the bucket's last application state.
/// This record survives deletion of the corresponding business entity.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "bucket_sync")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub bucket_name: String,
    #[sea_orm(unique)]
    pub public_policy_id: Uuid,
    pub generation: i64,
    pub applied_generation: i64,
    #[sea_orm(column_type = "Text", nullable)]
    pub last_error: Option<String>,
    pub delete_requested: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert {
            if self.public_policy_id.is_not_set() {
                self.public_policy_id = Set(super::new_sync_id()?);
            }
        }
        Ok(self)
    }
}

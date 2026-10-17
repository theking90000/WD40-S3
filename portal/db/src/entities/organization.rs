use sea_orm::{Set, entity::prelude::*};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "organizations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::organization_member::Entity")]
    OrganizationMember,
}

impl Related<super::organization_member::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::OrganizationMember.def()
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert && self.id.is_not_set() {
            let mut bytes = [0; 16];
            getrandom::fill(&mut bytes).map_err(|error| {
                DbErr::Custom(format!("cannot generate organization UUID: {error}"))
            })?;
            self.id = Set(uuid::Builder::from_random_bytes(bytes).into_uuid());
        }
        Ok(self)
    }
}

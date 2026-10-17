use sea_orm_migration::prelude::*;

mod m20261017_000001_create_organizations;
mod m20261017_000002_create_organization_members;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20261017_000001_create_organizations::Migration),
            Box::new(m20261017_000002_create_organization_members::Migration),
        ]
    }
}

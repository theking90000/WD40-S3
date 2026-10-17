//! Portal database entities and versioned migrations.
//!
//! SQLite is enabled by default; PostgreSQL is available with `postgres`.
//! The caller owns connection setup, migrations and shutdown.

mod entities;
mod migrations;

pub use entities::organization::{self, Model as Organization};
pub use entities::organization_member::{
    self, Model as OrganizationMember, OrganizationMemberAccess,
};
pub use migrations::Migrator;
pub use sea_orm::{self, ConnectOptions, Database, DatabaseConnection, DbErr};
pub use sea_orm_migration::MigratorTrait;

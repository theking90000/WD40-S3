//! Portal database entities and migrations belong in this crate.
//!
//! SQLite is enabled by default; PostgreSQL is available with `postgres`.
//! The caller owns connection setup, migrations and shutdown.

pub use sea_orm::{self, ConnectOptions, Database, DatabaseConnection, DbErr};

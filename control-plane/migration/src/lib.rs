//! SeaORM migrations for the control plane. Each migration is portable across
//! Postgres (prod) and SQLite (dev): string-valued enums, no PG-only column types.
pub use sea_orm_migration::prelude::*;

mod m20260601_000001_create_account;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20260601_000001_create_account::Migration)]
    }
}

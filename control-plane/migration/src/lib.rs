//! SeaORM migrations for the control plane. Each migration is portable across
//! Postgres (prod) and SQLite (dev): string-valued enums, no PG-only column types.
pub use sea_orm_migration::prelude::*;

mod m20260601_000001_create_account;
mod m20260601_000002_create_auth_tables;
mod m20260601_000003_create_subscription;
mod m20260601_000004_create_registry;
mod m20260601_000005_create_usage_adapter;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260601_000001_create_account::Migration),
            Box::new(m20260601_000002_create_auth_tables::Migration),
            Box::new(m20260601_000003_create_subscription::Migration),
            Box::new(m20260601_000004_create_registry::Migration),
            Box::new(m20260601_000005_create_usage_adapter::Migration),
        ]
    }
}

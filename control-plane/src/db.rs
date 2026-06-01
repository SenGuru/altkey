//! Database connection + boot-time migration runner. `connect` builds a SeaORM
//! `DatabaseConnection` from the config URL (SQLite or Postgres).
use crate::config::Config;
use anyhow::Result;
use migration::{Migrator, MigratorTrait};
use sea_orm::{Database, DatabaseConnection};

pub async fn connect(config: &Config) -> Result<DatabaseConnection> {
    let db = Database::connect(&config.database_url).await?;
    Ok(db)
}

pub async fn run_migrations(db: &DatabaseConnection) -> Result<()> {
    Migrator::up(db, None).await?;
    Ok(())
}

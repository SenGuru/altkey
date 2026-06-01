use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Handle { Table, Id, AccountId, Name, Status, CreatedAt }
#[derive(DeriveIden)]
#[allow(clippy::enum_variant_names)] // variant names map 1:1 to SQL columns
enum Agent { Table, Id, AccountId, HandleId, Name, AgentTokenHash, TokenPrefix, Status, CreatedAt, LastSeenAt }
#[derive(DeriveIden)]
enum EndpointKey { Table, Id, AccountId, AgentId, KeyHash, KeyPrefix, Name, CreatedAt, LastUsedAt, RevokedAt }

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(Table::create().table(Handle::Table).if_not_exists()
            .col(ColumnDef::new(Handle::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(Handle::AccountId).uuid().not_null())
            .col(ColumnDef::new(Handle::Name).string().not_null())
            .col(ColumnDef::new(Handle::Status).string().not_null().default("active"))
            .col(ColumnDef::new(Handle::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
            .to_owned()).await?;
        manager.create_index(Index::create().name("idx_handle_name_unique").table(Handle::Table).col(Handle::Name).unique().to_owned()).await?;

        manager.create_table(Table::create().table(Agent::Table).if_not_exists()
            .col(ColumnDef::new(Agent::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(Agent::AccountId).uuid().not_null())
            .col(ColumnDef::new(Agent::HandleId).uuid().not_null())
            .col(ColumnDef::new(Agent::Name).string().not_null())
            .col(ColumnDef::new(Agent::AgentTokenHash).string().not_null())
            .col(ColumnDef::new(Agent::TokenPrefix).string().not_null())
            .col(ColumnDef::new(Agent::Status).string().not_null().default("active"))
            .col(ColumnDef::new(Agent::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
            .col(ColumnDef::new(Agent::LastSeenAt).timestamp_with_time_zone().null())
            .to_owned()).await?;
        manager.create_index(Index::create().name("idx_agent_token_hash_unique").table(Agent::Table).col(Agent::AgentTokenHash).unique().to_owned()).await?;

        manager.create_table(Table::create().table(EndpointKey::Table).if_not_exists()
            .col(ColumnDef::new(EndpointKey::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(EndpointKey::AccountId).uuid().not_null())
            .col(ColumnDef::new(EndpointKey::AgentId).uuid().null())
            .col(ColumnDef::new(EndpointKey::KeyHash).string().not_null())
            .col(ColumnDef::new(EndpointKey::KeyPrefix).string().not_null())
            .col(ColumnDef::new(EndpointKey::Name).string().not_null())
            .col(ColumnDef::new(EndpointKey::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
            .col(ColumnDef::new(EndpointKey::LastUsedAt).timestamp_with_time_zone().null())
            .col(ColumnDef::new(EndpointKey::RevokedAt).timestamp_with_time_zone().null())
            .to_owned()).await?;
        manager.create_index(Index::create().name("idx_endpoint_key_hash_unique").table(EndpointKey::Table).col(EndpointKey::KeyHash).unique().to_owned()).await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(EndpointKey::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Agent::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Handle::Table).to_owned()).await
    }
}

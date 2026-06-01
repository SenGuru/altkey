use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum UsageRecord { Table, Id, AccountId, AgentId, KeyPrefix, Ts, Provider, Model, PromptTokens, CompletionTokens, TotalTokens, TunnelBytes, Tool }
#[derive(DeriveIden)]
enum UsageRollup { Table, Id, AccountId, Period, Model, Tool, Provider, SumRequests, SumTokens, SumBytes }
#[derive(DeriveIden)]
enum Adapter { Table, Id, Slug, Name, Description, Version, TargetTool, Manifest, PublishedAt }

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(Table::create().table(UsageRecord::Table).if_not_exists()
            .col(ColumnDef::new(UsageRecord::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(UsageRecord::AccountId).uuid().not_null())
            .col(ColumnDef::new(UsageRecord::AgentId).uuid().null())
            .col(ColumnDef::new(UsageRecord::KeyPrefix).string().null())
            .col(ColumnDef::new(UsageRecord::Ts).timestamp_with_time_zone().not_null())
            .col(ColumnDef::new(UsageRecord::Provider).string().not_null())
            .col(ColumnDef::new(UsageRecord::Model).string().not_null())
            .col(ColumnDef::new(UsageRecord::PromptTokens).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRecord::CompletionTokens).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRecord::TotalTokens).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRecord::TunnelBytes).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRecord::Tool).string().null())
            .to_owned()).await?;
        m.create_index(Index::create().name("idx_usage_record_account_ts").table(UsageRecord::Table).col(UsageRecord::AccountId).col(UsageRecord::Ts).to_owned()).await?;

        m.create_table(Table::create().table(UsageRollup::Table).if_not_exists()
            .col(ColumnDef::new(UsageRollup::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(UsageRollup::AccountId).uuid().not_null())
            .col(ColumnDef::new(UsageRollup::Period).string().not_null()) // "YYYY-MM-DD"
            .col(ColumnDef::new(UsageRollup::Model).string().null())
            .col(ColumnDef::new(UsageRollup::Tool).string().null())
            .col(ColumnDef::new(UsageRollup::Provider).string().null())
            .col(ColumnDef::new(UsageRollup::SumRequests).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRollup::SumTokens).big_integer().not_null().default(0))
            .col(ColumnDef::new(UsageRollup::SumBytes).big_integer().not_null().default(0))
            .to_owned()).await?;
        m.create_index(Index::create().name("idx_usage_rollup_account_period").table(UsageRollup::Table).col(UsageRollup::AccountId).col(UsageRollup::Period).to_owned()).await?;

        m.create_table(Table::create().table(Adapter::Table).if_not_exists()
            .col(ColumnDef::new(Adapter::Id).uuid().not_null().primary_key())
            .col(ColumnDef::new(Adapter::Slug).string().not_null())
            .col(ColumnDef::new(Adapter::Name).string().not_null())
            .col(ColumnDef::new(Adapter::Description).string().not_null().default(""))
            .col(ColumnDef::new(Adapter::Version).string().not_null().default("1.0.0"))
            .col(ColumnDef::new(Adapter::TargetTool).string().null())
            .col(ColumnDef::new(Adapter::Manifest).json().not_null())
            .col(ColumnDef::new(Adapter::PublishedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
            .to_owned()).await?;
        m.create_index(Index::create().name("idx_adapter_slug_unique").table(Adapter::Table).col(Adapter::Slug).unique().to_owned()).await
    }
    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(Adapter::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(UsageRollup::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(UsageRecord::Table).to_owned()).await
    }
}

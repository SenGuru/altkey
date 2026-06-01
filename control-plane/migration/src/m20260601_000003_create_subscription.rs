use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Subscription {
    Table,
    Id,
    AccountId,
    PolarCustomerId,
    PolarSubscriptionId,
    Plan,
    Status,
    CurrentPeriodEnd,
    IsFounding,
    CreatedAt,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(
            Table::create().table(Subscription::Table).if_not_exists()
                .col(ColumnDef::new(Subscription::Id).uuid().not_null().primary_key())
                .col(ColumnDef::new(Subscription::AccountId).uuid().not_null())
                .col(ColumnDef::new(Subscription::PolarCustomerId).string().null())
                .col(ColumnDef::new(Subscription::PolarSubscriptionId).string().null())
                .col(ColumnDef::new(Subscription::Plan).string().not_null())
                .col(ColumnDef::new(Subscription::Status).string().not_null())
                .col(ColumnDef::new(Subscription::CurrentPeriodEnd).timestamp_with_time_zone().null())
                .col(ColumnDef::new(Subscription::IsFounding).boolean().not_null().default(false))
                .col(ColumnDef::new(Subscription::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
                .col(ColumnDef::new(Subscription::UpdatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
                .to_owned(),
        ).await?;
        // One active subscription row per account (we upsert by account).
        manager.create_index(
            Index::create().name("idx_subscription_account_unique")
                .table(Subscription::Table).col(Subscription::AccountId).unique().to_owned(),
        ).await?;
        // Look up by Polar subscription id from webhooks.
        manager.create_index(
            Index::create().name("idx_subscription_polar_sub")
                .table(Subscription::Table).col(Subscription::PolarSubscriptionId).to_owned(),
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Subscription::Table).to_owned()).await
    }
}

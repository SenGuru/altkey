//! SeaORM entity for `subscription` — the license gate. One row per account.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "subscription")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub account_id: Uuid,
    pub polar_customer_id: Option<String>,
    pub polar_subscription_id: Option<String>,
    pub plan: String,
    pub status: String,
    pub current_period_end: Option<DateTimeWithTimeZone>,
    pub is_founding: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

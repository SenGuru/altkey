//! SeaORM entity for the `usage_rollup` table — per-account/day/model aggregates.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "usage_rollup")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub account_id: Uuid,
    pub period: String, // "YYYY-MM-DD"
    pub model: Option<String>,
    pub tool: Option<String>,
    pub provider: Option<String>,
    pub sum_requests: i64,
    pub sum_tokens: i64,
    pub sum_bytes: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

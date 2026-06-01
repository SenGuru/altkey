//! SeaORM entity for the `agent` table — a paired machine, identified by an ak_agent_ token.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "agent")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub account_id: Uuid,
    pub handle_id: Uuid,
    pub name: String,
    #[sea_orm(unique)]
    pub agent_token_hash: String,
    pub token_prefix: String,
    pub status: String,
    pub created_at: DateTimeWithTimeZone,
    pub last_seen_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

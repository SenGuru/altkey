//! SeaORM entity for the `endpoint_key` table — an ak_live_ key used to authenticate requests.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "endpoint_key")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub account_id: Uuid,
    pub agent_id: Option<Uuid>,
    #[sea_orm(unique)]
    pub key_hash: String,
    pub key_prefix: String,
    pub name: String,
    pub created_at: DateTimeWithTimeZone,
    pub last_used_at: Option<DateTimeWithTimeZone>,
    pub revoked_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

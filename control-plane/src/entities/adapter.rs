//! SeaORM entity for the `adapter` table — the adapter catalog.
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "adapter")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub slug: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub target_tool: Option<String>,
    #[sea_orm(column_type = "Json")]
    pub manifest: Json,
    pub published_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

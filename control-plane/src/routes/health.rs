//! Liveness probe + a tiny DB ping so /health reflects DB connectivity.
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use sea_orm::{ConnectionTrait, Statement};
use serde::Serialize;

#[derive(Serialize, utoipa::ToSchema)]
pub struct Health {
    pub status: String,
    pub db: bool,
}

#[utoipa::path(
    get,
    path = "/health",
    responses((status = 200, description = "Service health", body = Health)),
    tag = "system"
)]
pub async fn health(State(state): State<AppState>) -> Json<Health> {
    let db_ok = state
        .db
        .execute(Statement::from_string(
            state.db.get_database_backend(),
            "SELECT 1".to_owned(),
        ))
        .await
        .is_ok();
    Json(Health { status: "ok".into(), db: db_ok })
}

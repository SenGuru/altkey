//! Aggregate raw usage_record rows into per-account/day/model usage_rollup rows.
//! Idempotent: clears an account's rollups for the affected days then rewrites them.
//! Simple in-Rust aggregation (portable across SQLite + Postgres).
use crate::entities::{prelude::*, usage_record, usage_rollup};
use anyhow::Result;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::collections::HashMap;
use uuid::Uuid;

/// Recompute rollups for one account from its raw records.
pub async fn rebuild_for_account(db: &DatabaseConnection, account_id: Uuid) -> Result<()> {
    let records = UsageRecord::find()
        .filter(usage_record::Column::AccountId.eq(account_id))
        .all(db)
        .await?;

    // key: (period_yyyy_mm_dd, model, tool, provider)
    let mut agg: HashMap<(String, String, String, String), (i64, i64, i64)> = HashMap::new();
    for r in &records {
        let period = chrono::DateTime::<chrono::Utc>::from(r.ts).format("%Y-%m-%d").to_string();
        let key = (
            period,
            r.model.clone(),
            r.tool.clone().unwrap_or_default(),
            r.provider.clone(),
        );
        let e = agg.entry(key).or_insert((0, 0, 0));
        e.0 += 1;
        e.1 += r.total_tokens;
        e.2 += r.tunnel_bytes;
    }

    // Clear existing rollups for the account, then rewrite.
    UsageRollup::delete_many()
        .filter(usage_rollup::Column::AccountId.eq(account_id))
        .exec(db)
        .await?;

    for ((period, model, tool, provider), (reqs, tokens, bytes)) in agg {
        usage_rollup::ActiveModel {
            id: Set(Uuid::new_v4()),
            account_id: Set(account_id),
            period: Set(period),
            model: Set(if model.is_empty() { None } else { Some(model) }),
            tool: Set(if tool.is_empty() { None } else { Some(tool) }),
            provider: Set(if provider.is_empty() { None } else { Some(provider) }),
            sum_requests: Set(reqs),
            sum_tokens: Set(tokens),
            sum_bytes: Set(bytes),
        }
        .insert(db)
        .await?;
    }
    Ok(())
}

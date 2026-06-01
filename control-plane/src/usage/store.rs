//! Append usage records ingested from agents. Never fails a request — caller acks
//! regardless; bad rows are skipped.
use crate::entities::usage_record;
use altkey_api::dto::UsageRecordDto;
use anyhow::Result;
use chrono::DateTime;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use uuid::Uuid;

pub async fn insert_records(
    db: &DatabaseConnection,
    account_id: Uuid,
    agent_id: Option<Uuid>,
    records: &[UsageRecordDto],
) -> Result<usize> {
    let mut n = 0;
    for r in records {
        let ts = DateTime::parse_from_rfc3339(&r.ts)
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let ok = usage_record::ActiveModel {
            id: Set(Uuid::new_v4()),
            account_id: Set(account_id),
            agent_id: Set(agent_id),
            key_prefix: Set(r.key_prefix.clone()),
            ts: Set(ts.into()),
            provider: Set(r.provider.clone()),
            model: Set(r.model.clone()),
            prompt_tokens: Set(r.prompt_tokens),
            completion_tokens: Set(r.completion_tokens),
            total_tokens: Set(r.total_tokens),
            tunnel_bytes: Set(r.tunnel_bytes),
            tool: Set(r.tool.clone()),
        }
        .insert(db)
        .await;
        if ok.is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

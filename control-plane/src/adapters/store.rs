//! Adapter catalog store: list, get by slug, and idempotent seed.
use crate::entities::{adapter, prelude::Adapter};
use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use uuid::Uuid;

/// Return all adapters in the catalog.
pub async fn list(db: &DatabaseConnection) -> Result<Vec<adapter::Model>> {
    Ok(Adapter::find().all(db).await?)
}

/// Return a single adapter by slug, or `None` if not found.
pub async fn get(db: &DatabaseConnection, slug: &str) -> Result<Option<adapter::Model>> {
    use crate::entities::adapter::Column;
    use sea_orm::ColumnTrait;
    use sea_orm::QueryFilter;
    Ok(Adapter::find()
        .filter(Column::Slug.eq(slug))
        .one(db)
        .await?)
}

/// Insert starter adapters if the table is empty (idempotent — no-op if any row exists).
pub async fn seed_defaults(db: &DatabaseConnection) -> Result<()> {
    // Only seed when the table is empty.
    let count = Adapter::find().all(db).await?.len();
    if count > 0 {
        return Ok(());
    }

    // Starter adapter 1: OpenAI Base URL Shim
    adapter::ActiveModel {
        id: Set(Uuid::new_v4()),
        slug: Set("openai-base-url".into()),
        name: Set("OpenAI Base URL Shim".into()),
        description: Set(
            "Redirects OPENAI_BASE_URL to the altkey tunnel so any OpenAI-compatible \
             tool uses the managed key without touching its source code."
                .into(),
        ),
        version: Set("1.0.0".into()),
        target_tool: Set(Some("generic".into())),
        manifest: Set(serde_json::json!({
            "kind": "base_url",
            "note": "point OPENAI_BASE_URL at the agent"
        })),
        published_at: Set(Utc::now().into()),
    }
    .insert(db)
    .await?;

    // Starter adapter 2: Claude Code Proxy Shim
    adapter::ActiveModel {
        id: Set(Uuid::new_v4()),
        slug: Set("claude-code-proxy".into()),
        name: Set("Claude Code Proxy Shim".into()),
        description: Set(
            "Injects the altkey tunnel URL into Claude Code's \
             ANTHROPIC_BASE_URL so the agent token is used transparently."
                .into(),
        ),
        version: Set("1.0.0".into()),
        target_tool: Set(Some("claude-code".into())),
        manifest: Set(serde_json::json!({
            "kind": "base_url",
            "env": "ANTHROPIC_BASE_URL",
            "note": "set ANTHROPIC_BASE_URL to the tunnel base path"
        })),
        published_at: Set(Utc::now().into()),
    }
    .insert(db)
    .await?;

    Ok(())
}

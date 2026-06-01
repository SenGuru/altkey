//! Registry writes/reads: claim a handle, pair an agent (mint ak_agent_), mint an
//! ak_live_ key. Secrets are returned in plaintext ONCE; only the hash is stored.
use crate::entities::{agent, endpoint_key, handle, prelude::*};
use altkey_api::token::{self, TokenKind};
use anyhow::{anyhow, Result};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

/// Valid handle names: lowercase alphanumeric + hyphens, 1–63 chars, no leading/trailing hyphen.
pub fn valid_handle_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && n.len() <= 63
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !n.starts_with('-')
        && !n.ends_with('-')
}

/// True if the handle name is not yet taken (or revoked — uniqueness is by name string).
pub async fn handle_available(db: &DatabaseConnection, name: &str) -> Result<bool> {
    Ok(Handle::find()
        .filter(handle::Column::Name.eq(name))
        .one(db)
        .await?
        .is_none())
}

/// Claim a handle for `account_id`. Fails if the name is invalid or already taken.
pub async fn claim_handle(
    db: &DatabaseConnection,
    account_id: Uuid,
    name: &str,
) -> Result<handle::Model> {
    let name = name.trim().to_lowercase();
    if !valid_handle_name(&name) {
        return Err(anyhow!("invalid handle name"));
    }
    if !handle_available(db, &name).await? {
        return Err(anyhow!("handle taken"));
    }
    Ok(handle::ActiveModel {
        id: Set(Uuid::new_v4()),
        account_id: Set(account_id),
        name: Set(name),
        status: Set("active".into()),
        created_at: Set(Utc::now().into()),
    }
    .insert(db)
    .await?)
}

pub struct PairedAgent {
    pub agent: agent::Model,
    pub token_plaintext: String,
}

/// Pair a new agent to a handle. The caller must own the handle (account_id must match).
/// Returns the agent model + the plaintext token (shown ONCE; only the hash is stored).
pub async fn pair_agent(
    db: &DatabaseConnection,
    account_id: Uuid,
    handle_id: Uuid,
    name: &str,
) -> Result<PairedAgent> {
    let h = Handle::find_by_id(handle_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow!("no such handle"))?;
    if h.account_id != account_id {
        return Err(anyhow!("handle not owned"));
    }
    let tok = token::generate(TokenKind::Agent);
    let model = agent::ActiveModel {
        id: Set(Uuid::new_v4()),
        account_id: Set(account_id),
        handle_id: Set(handle_id),
        name: Set(name.to_string()),
        agent_token_hash: Set(token::hash(&tok.secret)),
        token_prefix: Set(token::prefix(&tok.secret)),
        status: Set("active".into()),
        created_at: Set(Utc::now().into()),
        last_seen_at: Set(None),
    }
    .insert(db)
    .await?;
    Ok(PairedAgent {
        agent: model,
        token_plaintext: tok.secret,
    })
}

pub struct MintedKey {
    pub key: endpoint_key::Model,
    pub key_plaintext: String,
}

/// Mint a new ak_live_ endpoint key for `account_id`. `agent_id` is optional.
/// Returns the key model + the plaintext secret (shown ONCE; only the hash is stored).
pub async fn mint_key(
    db: &DatabaseConnection,
    account_id: Uuid,
    agent_id: Option<Uuid>,
    name: &str,
) -> Result<MintedKey> {
    let tok = token::generate(TokenKind::Live);
    let model = endpoint_key::ActiveModel {
        id: Set(Uuid::new_v4()),
        account_id: Set(account_id),
        agent_id: Set(agent_id),
        key_hash: Set(token::hash(&tok.secret)),
        key_prefix: Set(token::prefix(&tok.secret)),
        name: Set(name.to_string()),
        created_at: Set(Utc::now().into()),
        last_used_at: Set(None),
        revoked_at: Set(None),
    }
    .insert(db)
    .await?;
    Ok(MintedKey {
        key: model,
        key_plaintext: tok.secret,
    })
}

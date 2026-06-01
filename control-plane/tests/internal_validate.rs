//! Truth-table test for the internal validation endpoints.
//!
//! Seeds: account + active subscription + handle + agent + endpoint key.
//! Calls handlers directly (State-inject, same as billing_webhook.rs) so there's
//! no network overhead and the test remains self-contained.
//!
//! Truth table:
//!   authorize without service secret             → 401
//!   authorize valid (secret + agent + handle + sub) → ok=true
//!   authorize wrong handle (not owned)           → ok=false (200)
//!   authorize unknown agent token                → ok=false (200)
//!   after canceling sub → authorize              → ok=false (200)
//!   key_validate valid key + agent + active sub  → valid=true, sub_active=true
//!   key_validate revoked key                     → valid=false
//!   key_validate key from different account      → valid=false
use control_plane::billing::plan::Plan;
use control_plane::billing::polar::FakePolarClient;
use control_plane::billing::store::{upsert_from_polar, PolarSubscriptionEvent};
use control_plane::config::Config;
use control_plane::entities::{account, endpoint_key};
use control_plane::internal::routes::{authorize, heartbeat, key_validate, Heartbeat};
use control_plane::registry::store::{claim_handle, mint_key, pair_agent};
use control_plane::state::AppState;
use altkey_api::dto::{AuthorizeRequest, KeyValidateRequest};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Utc;
use migration::MigratorTrait;
use sea_orm::{ActiveModelTrait, Database, EntityTrait, Set};
use std::sync::Arc;
use uuid::Uuid;

const SVC_SECRET: &str = "svc-secret";

async fn make_state() -> (AppState, sea_orm::DatabaseConnection) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    migration::Migrator::up(&db, None).await.unwrap();
    let config = Config {
        database_url: "sqlite::memory:".into(),
        public_base_url: "http://localhost".into(),
        internal_service_secret: Some(SVC_SECRET.into()),
        bind_addr: "127.0.0.1:0".into(),
        polar_access_token: None,
        polar_webhook_secret: None,
        polar_base_url: "https://api.polar.sh".into(),
        polar_product_founding: None,
        polar_product_standard: Some("prod_standard".into()),
        polar_product_pro: None,
    };
    let st = AppState {
        db: db.clone(),
        config,
        email: Arc::new(control_plane::auth::email::LoggingEmailSender),
        oauth: Arc::new(control_plane::auth::oauth::OAuthRegistry::default()),
        polar: Arc::new(FakePolarClient),
    };
    (st, db)
}

fn headers_with_secret() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("x-altkey-service-secret", SVC_SECRET.parse().unwrap());
    h
}

fn headers_no_secret() -> HeaderMap {
    HeaderMap::new()
}

fn headers_wrong_secret() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("x-altkey-service-secret", "wrong-secret".parse().unwrap());
    h
}

async fn seed_sub(db: &sea_orm::DatabaseConnection, account_id: Uuid, status: &str) {
    upsert_from_polar(
        db,
        &PolarSubscriptionEvent {
            account_id,
            polar_customer_id: Some("cus_test".into()),
            polar_subscription_id: Some("sub_test".into()),
            plan: Plan::Standard,
            status: status.to_string(),
            current_period_end: Some(Utc::now() + chrono::Duration::days(30)),
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn internal_validate_truth_table() {
    let (st, db) = make_state().await;

    // ── Seed account ──────────────────────────────────────────────────────────
    let account_id = Uuid::new_v4();
    account::ActiveModel {
        id: Set(account_id),
        email: Set("val@example.com".into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(Utc::now().into()),
    }
    .insert(&db)
    .await
    .unwrap();

    // Active subscription
    seed_sub(&db, account_id, "active").await;

    // Claim a handle
    let handle = claim_handle(&db, account_id, "val-handle").await.unwrap();

    // Pair an agent to that handle
    let pa = pair_agent(&db, account_id, handle.id, "test-agent")
        .await
        .unwrap();
    let agent_token = pa.token_plaintext.clone();

    // Mint a key bound to this account (and agent)
    let mk = mint_key(&db, account_id, Some(pa.agent.id), "prod-key")
        .await
        .unwrap();
    let key_secret = mk.key_plaintext.clone();

    // ── 1. authorize WITHOUT service secret → 401 ────────────────────────────
    let (status, _) = authorize(
        State(st.clone()),
        headers_no_secret(),
        Json(AuthorizeRequest {
            handle: "val-handle".into(),
            agent_token: agent_token.clone(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "missing secret must return 401");

    // ── 2. authorize with WRONG secret → 401 ─────────────────────────────────
    let (status, _) = authorize(
        State(st.clone()),
        headers_wrong_secret(),
        Json(AuthorizeRequest {
            handle: "val-handle".into(),
            agent_token: agent_token.clone(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "wrong secret must return 401");

    // ── 3. authorize valid (correct secret + agent + owned handle + active sub) → ok=true
    let (status, Json(resp)) = authorize(
        State(st.clone()),
        headers_with_secret(),
        Json(AuthorizeRequest {
            handle: "val-handle".into(),
            agent_token: agent_token.clone(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(resp.ok, "valid authorize must return ok=true");
    assert_eq!(resp.account_id, account_id.to_string());
    assert!(!resp.plan.is_empty(), "plan must be set on success");

    // ── 4. authorize with an unknown agent token → ok=false ──────────────────
    let (status, Json(resp)) = authorize(
        State(st.clone()),
        headers_with_secret(),
        Json(AuthorizeRequest {
            handle: "val-handle".into(),
            agent_token: "ak_agent_unknowntokenxxxxxxxxxxxxxxxxxxxxxxxxxxx".into(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!resp.ok, "unknown agent must return ok=false");

    // ── 5. authorize with a handle the agent does NOT own → ok=false ─────────
    // Seed a second account + handle; use the first agent's token with the second handle.
    let account2_id = Uuid::new_v4();
    account::ActiveModel {
        id: Set(account2_id),
        email: Set("other@example.com".into()),
        display_name: Set(None),
        status: Set("active".into()),
        created_at: Set(Utc::now().into()),
    }
    .insert(&db)
    .await
    .unwrap();
    seed_sub(&db, account2_id, "active").await;
    claim_handle(&db, account2_id, "other-handle").await.unwrap();

    let (status, Json(resp)) = authorize(
        State(st.clone()),
        headers_with_secret(),
        Json(AuthorizeRequest {
            handle: "other-handle".into(), // handle belongs to account2, agent belongs to account1
            agent_token: agent_token.clone(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!resp.ok, "cross-account handle must return ok=false");

    // ── 6. authorize after canceling the subscription → ok=false ─────────────
    seed_sub(&db, account_id, "canceled").await;

    let (status, Json(resp)) = authorize(
        State(st.clone()),
        headers_with_secret(),
        Json(AuthorizeRequest {
            handle: "val-handle".into(),
            agent_token: agent_token.clone(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!resp.ok, "canceled sub must return ok=false");

    // Restore active sub for the key_validate tests.
    seed_sub(&db, account_id, "active").await;

    // ── 7. key_validate valid key + agent + active sub → valid=true, sub_active=true
    let Json(kv) = key_validate(
        State(st.clone()),
        Json(KeyValidateRequest {
            key: key_secret.clone(),
            agent_token: agent_token.clone(),
        }),
    )
    .await;
    assert!(kv.valid, "valid key must return valid=true");
    assert!(kv.sub_active, "active sub must return sub_active=true");
    assert!(!kv.plan.is_empty(), "plan must be set when valid");

    // ── 8. key_validate with a revoked key → valid=false ─────────────────────
    // Revoke the key by setting revoked_at.
    use control_plane::entities::prelude::EndpointKey;
    let key_row: endpoint_key::Model = EndpointKey::find_by_id(mk.key.id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    let mut am: endpoint_key::ActiveModel = key_row.into();
    am.revoked_at = Set(Some(Utc::now().into()));
    am.update(&db).await.unwrap();

    let Json(kv) = key_validate(
        State(st.clone()),
        Json(KeyValidateRequest {
            key: key_secret.clone(),
            agent_token: agent_token.clone(),
        }),
    )
    .await;
    assert!(!kv.valid, "revoked key must return valid=false");

    // ── 9. key_validate key from DIFFERENT account than agent → valid=false ───
    // Mint a new key for account2, then try to validate it with account1's agent token.
    let mk2 = mint_key(&db, account2_id, None, "other-key")
        .await
        .unwrap();

    let Json(kv) = key_validate(
        State(st.clone()),
        Json(KeyValidateRequest {
            key: mk2.key_plaintext.clone(),
            agent_token: agent_token.clone(), // agent1 belongs to account1
        }),
    )
    .await;
    assert!(!kv.valid, "cross-account key must return valid=false");

    // ── 10. heartbeat: known token → 200 ─────────────────────────────────────
    let status = heartbeat(
        State(st.clone()),
        Json(Heartbeat { agent_token: agent_token.clone() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "heartbeat with valid token must return 200");

    // ── 11. heartbeat: unknown token → still 200 (best-effort, don't leak) ───
    let status = heartbeat(
        State(st.clone()),
        Json(Heartbeat {
            agent_token: "ak_agent_unknownxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx".into(),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "heartbeat with unknown token must still return 200");
}

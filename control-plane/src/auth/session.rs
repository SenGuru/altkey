//! Opaque session tokens stored hashed. The cookie carries the plaintext token;
//! the DB stores only its sha256. Lookups join to the account and check expiry.
use crate::entities::{account, prelude::Account, prelude::Session, session};
use altkey_api::token;
use anyhow::Result;
use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{Duration, Utc};
use rand::Rng;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "altkey_session";
const SESSION_DAYS: i64 = 30;

fn random_token() -> String {
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..48).map(|_| A[rng.gen_range(0..A.len())] as char).collect()
}

/// Create a session for `account_id`; returns the plaintext token (store in a cookie).
pub async fn issue(db: &DatabaseConnection, account_id: Uuid) -> Result<String> {
    let plaintext = random_token();
    session::ActiveModel {
        id: Set(Uuid::new_v4()),
        account_id: Set(account_id),
        token_hash: Set(token::hash(&plaintext)),
        created_at: Set(Utc::now().into()),
        expires_at: Set((Utc::now() + Duration::days(SESSION_DAYS)).into()),
        last_seen_at: Set(None),
    }
    .insert(db)
    .await?;
    Ok(plaintext)
}

/// Resolve a plaintext session token to its account, if the session exists and is unexpired.
pub async fn account_for(db: &DatabaseConnection, plaintext: &str) -> Result<Option<account::Model>> {
    let hash = token::hash(plaintext);
    let Some(s) = Session::find()
        .filter(session::Column::TokenHash.eq(hash))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    if s.expires_at < Utc::now() {
        return Ok(None);
    }
    Ok(Account::find_by_id(s.account_id).one(db).await?)
}

/// Revoke (delete) the session for a plaintext token. Idempotent.
pub async fn revoke(db: &DatabaseConnection, plaintext: &str) -> Result<()> {
    let hash = token::hash(plaintext);
    Session::delete_many()
        .filter(session::Column::TokenHash.eq(hash))
        .exec(db)
        .await?;
    Ok(())
}

/// Build the Set-Cookie for a freshly issued session token.
pub fn cookie_for(token: String) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::days(SESSION_DAYS))
        .build()
}

/// Build the Set-Cookie that clears the session (logout).
pub fn clear_cookie() -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, ""))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(0))
        .build()
}

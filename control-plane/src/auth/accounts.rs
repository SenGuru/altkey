//! Upsert an account by email and ensure a linked identity row exists. Email is
//! the identity key, so Google-then-GitHub on the same address is ONE account.
use crate::entities::{account, identity, prelude::Account, prelude::Identity};
use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

pub async fn upsert_account_with_identity(
    db: &DatabaseConnection,
    email: &str,
    provider: &str,
    provider_uid: &str,
) -> Result<account::Model> {
    let email = email.trim().to_lowercase();

    let acct = match Account::find()
        .filter(account::Column::Email.eq(email.clone()))
        .one(db)
        .await?
    {
        Some(a) => a,
        None => {
            account::ActiveModel {
                id: Set(Uuid::new_v4()),
                email: Set(email.clone()),
                display_name: Set(None),
                status: Set("active".into()),
                created_at: Set(Utc::now().into()),
            }
            .insert(db)
            .await?
        }
    };

    let existing = Identity::find()
        .filter(identity::Column::Provider.eq(provider))
        .filter(identity::Column::ProviderUserId.eq(provider_uid))
        .one(db)
        .await?;
    if existing.is_none() {
        identity::ActiveModel {
            id: Set(Uuid::new_v4()),
            account_id: Set(acct.id),
            provider: Set(provider.to_string()),
            provider_user_id: Set(provider_uid.to_string()),
            email_at_provider: Set(Some(email.clone())),
            created_at: Set(Utc::now().into()),
        }
        .insert(db)
        .await?;
    }
    Ok(acct)
}

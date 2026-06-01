//! The license gate: read/write subscription state. `active_subscription` is the
//! single source of truth every other part of altkey reads.
use crate::billing::plan::Plan;
use crate::entities::{prelude::Subscription, subscription};
use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

/// The event shape we extract from a Polar subscription webhook (provider-agnostic
/// here so the webhook parser owns Polar's JSON shape).
pub struct PolarSubscriptionEvent {
    pub account_id: Uuid,
    pub polar_customer_id: Option<String>,
    pub polar_subscription_id: Option<String>,
    pub plan: Plan,
    pub status: String, // "active" | "trialing" | "past_due" | "canceled"
    pub current_period_end: Option<chrono::DateTime<Utc>>,
}

/// Return the account's subscription IF it is active/trialing and not past its period.
pub async fn active_subscription(
    db: &DatabaseConnection,
    account_id: Uuid,
) -> Result<Option<subscription::Model>> {
    let Some(s) = Subscription::find()
        .filter(subscription::Column::AccountId.eq(account_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let live = matches!(s.status.as_str(), "active" | "trialing");
    let unexpired = s
        .current_period_end
        .map(|e| e > Utc::now())
        .unwrap_or(true);
    Ok((live && unexpired).then_some(s))
}

/// Upsert (by account_id) the subscription from a Polar webhook event.
pub async fn upsert_from_polar(
    db: &DatabaseConnection,
    ev: &PolarSubscriptionEvent,
) -> Result<()> {
    let now = Utc::now();
    let existing = Subscription::find()
        .filter(subscription::Column::AccountId.eq(ev.account_id))
        .one(db)
        .await?;
    match existing {
        Some(row) => {
            let mut am: subscription::ActiveModel = row.into();
            am.polar_customer_id = Set(ev.polar_customer_id.clone());
            am.polar_subscription_id = Set(ev.polar_subscription_id.clone());
            am.plan = Set(ev.plan.as_str().to_string());
            am.status = Set(ev.status.clone());
            am.current_period_end = Set(ev.current_period_end.map(|d| d.into()));
            am.is_founding = Set(ev.plan.is_founding());
            am.updated_at = Set(now.into());
            am.update(db).await?;
        }
        None => {
            subscription::ActiveModel {
                id: Set(Uuid::new_v4()),
                account_id: Set(ev.account_id),
                polar_customer_id: Set(ev.polar_customer_id.clone()),
                polar_subscription_id: Set(ev.polar_subscription_id.clone()),
                plan: Set(ev.plan.as_str().to_string()),
                status: Set(ev.status.clone()),
                current_period_end: Set(ev.current_period_end.map(|d| d.into())),
                is_founding: Set(ev.plan.is_founding()),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

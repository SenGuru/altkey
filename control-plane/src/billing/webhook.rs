//! POST /webhooks/polar — verify the Standard-Webhooks signature over the RAW body,
//! parse subscription events, and upsert the account's subscription. Our account id
//! travels in the checkout `metadata.account_id`, which Polar echoes on the
//! subscription object.
use crate::billing::plan::Plan;
use crate::billing::store::{upsert_from_polar, PolarSubscriptionEvent};
use crate::billing::webhook_sig::{verify, WebhookHeaders};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use chrono::DateTime;
use uuid::Uuid;

#[utoipa::path(post, path = "/webhooks/polar", tag = "billing",
    request_body(content = String, description = "Raw webhook payload", content_type = "application/json"),
    responses((status = 200, description = "Processed"), (status = 401, description = "Bad signature")))]
pub async fn polar_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let Some(secret) = state.config.polar_webhook_secret.clone() else {
        tracing::warn!("polar webhook received but POLAR_WEBHOOK_SECRET unset");
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let h = WebhookHeaders {
        id: header_val(&headers, "webhook-id"),
        timestamp: header_val(&headers, "webhook-timestamp"),
        signature: header_val(&headers, "webhook-signature"),
    };
    if verify(&secret, &h, &body).is_err() {
        return StatusCode::UNAUTHORIZED;
    }

    let Ok(ev) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    let typ = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if !typ.starts_with("subscription.") {
        return StatusCode::OK; // we only act on subscription.* events
    }
    match parse_subscription_event(&state, &ev) {
        Some(sub_ev) => {
            if let Err(e) = upsert_from_polar(&state.db, &sub_ev).await {
                tracing::error!("subscription upsert failed: {e:#}");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            StatusCode::OK
        }
        None => {
            tracing::warn!("subscription event missing account_id/product mapping: {typ}");
            StatusCode::OK // ack so Polar doesn't retry forever; logged for triage
        }
    }
}

fn header_val(h: &HeaderMap, name: &str) -> String {
    h.get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

/// Pull our PolarSubscriptionEvent out of Polar's webhook JSON. Maps Polar status
/// to our status vocabulary and product id → Plan.
fn parse_subscription_event(state: &AppState, ev: &serde_json::Value) -> Option<PolarSubscriptionEvent> {
    let data = ev.get("data")?;
    let account_id = data
        .get("metadata")
        .and_then(|m| m.get("account_id"))
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())?;
    let product_id = data
        .get("product_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            data.get("product")
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str())
        })?;
    let plan = Plan::from_polar_product(&state.config, product_id)?;

    let polar_status = data.get("status").and_then(|v| v.as_str()).unwrap_or("active");
    let status = map_status(polar_status).to_string();

    let current_period_end = data
        .get("current_period_end")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));

    Some(PolarSubscriptionEvent {
        account_id,
        polar_customer_id: data.get("customer_id").and_then(|v| v.as_str()).map(String::from),
        polar_subscription_id: data.get("id").and_then(|v| v.as_str()).map(String::from),
        plan,
        status,
        current_period_end,
    })
}

/// Normalize Polar's subscription status to our vocabulary.
fn map_status(polar: &str) -> &'static str {
    match polar {
        "active" => "active",
        "trialing" => "trialing",
        "past_due" | "unpaid" => "past_due",
        "canceled" | "revoked" | "incomplete_expired" => "canceled",
        _ => "active",
    }
}

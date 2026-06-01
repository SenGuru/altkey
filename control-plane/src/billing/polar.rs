//! Polar API client (checkout + customer portal). The real impl calls Polar's REST
//! API; the fake returns canned URLs so handlers + the webhook test run offline.
use crate::billing::plan::Plan;
use crate::config::Config;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use uuid::Uuid;

#[async_trait::async_trait]
pub trait PolarClient: Send + Sync {
    /// Create a checkout for `plan`, embedding our `account_id` in metadata so the
    /// webhook can map the resulting subscription back to the account. Returns the URL.
    async fn create_checkout(&self, account_id: Uuid, plan: Plan, success_url: &str) -> Result<String>;
    /// Return a customer-portal URL for managing/canceling a subscription.
    async fn customer_portal_url(&self, polar_customer_id: &str) -> Result<String>;
}

/// Real Polar REST client.
pub struct HttpPolarClient {
    pub config: Config,
    pub http: reqwest::Client,
}

impl HttpPolarClient {
    pub fn new(config: Config) -> Self {
        Self { config, http: reqwest::Client::new() }
    }
}

#[async_trait::async_trait]
impl PolarClient for HttpPolarClient {
    async fn create_checkout(&self, account_id: Uuid, plan: Plan, success_url: &str) -> Result<String> {
        let token = self.config.polar_access_token.as_ref()
            .ok_or_else(|| anyhow!("POLAR_ACCESS_TOKEN unset"))?;
        let product_id = plan.polar_product_id(&self.config)
            .ok_or_else(|| anyhow!("no polar product id for plan {:?}", plan))?;
        let body = serde_json::json!({
            "products": [product_id],
            "success_url": success_url,
            "metadata": { "account_id": account_id.to_string() }
        });
        let resp: serde_json::Value = self.http
            .post(format!("{}/v1/checkouts/", self.config.polar_base_url))
            .bearer_auth(token)
            .json(&body)
            .send().await?
            .error_for_status()?
            .json().await?;
        resp.get("url").and_then(|v| v.as_str()).map(String::from)
            .ok_or_else(|| anyhow!("polar checkout response missing url: {resp}"))
    }

    async fn customer_portal_url(&self, polar_customer_id: &str) -> Result<String> {
        let token = self.config.polar_access_token.as_ref()
            .ok_or_else(|| anyhow!("POLAR_ACCESS_TOKEN unset"))?;
        let resp: serde_json::Value = self.http
            .post(format!("{}/v1/customer-sessions/", self.config.polar_base_url))
            .bearer_auth(token)
            .json(&serde_json::json!({ "customer_id": polar_customer_id }))
            .send().await?
            .error_for_status()?
            .json().await?;
        resp.get("customer_portal_url").and_then(|v| v.as_str()).map(String::from)
            .ok_or_else(|| anyhow!("polar customer session missing portal url: {resp}"))
    }
}

/// Test fake: deterministic URLs, no network.
#[derive(Default)]
pub struct FakePolarClient;

#[async_trait::async_trait]
impl PolarClient for FakePolarClient {
    async fn create_checkout(&self, account_id: Uuid, plan: Plan, _success_url: &str) -> Result<String> {
        Ok(format!("https://polar.test/checkout/{}/{}", plan.as_str(), account_id))
    }
    async fn customer_portal_url(&self, polar_customer_id: &str) -> Result<String> {
        Ok(format!("https://polar.test/portal/{polar_customer_id}"))
    }
}

/// Build the configured client (real if a token is set, else the fake so dev boots).
pub fn from_config(config: &Config) -> Arc<dyn PolarClient> {
    if config.polar_access_token.is_some() {
        Arc::new(HttpPolarClient::new(config.clone()))
    } else {
        Arc::new(FakePolarClient)
    }
}

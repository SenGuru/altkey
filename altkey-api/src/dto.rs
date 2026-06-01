//! Shared internal-API request/response types used by the control plane,
//! relay, and engine. All types are serde-serializable and Clone.

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AuthorizeRequest {
    pub handle: String,
    pub agent_token: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Limits {
    pub max_concurrency: u32,
    pub max_rps: u32,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct AuthorizeResponse {
    pub ok: bool,
    pub account_id: String,
    pub plan: String,
    pub limits: Limits,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct KeyValidateRequest {
    pub key: String,
    pub agent_token: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct KeyValidateResponse {
    pub valid: bool,
    pub sub_active: bool,
    pub plan: String,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct UsageRecordDto {
    pub ts: String,          // RFC3339
    pub provider: String,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub tunnel_bytes: i64,
    pub tool: Option<String>,
    pub key_prefix: Option<String>, // which ak_live_ (prefix only)
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct UsageBatch {
    pub agent_token: String,
    pub records: Vec<UsageRecordDto>,
}

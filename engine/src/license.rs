//! Validate ak_live_ keys against the control plane, with a short positive cache
//! and a bounded offline-grace window so a control-plane blip doesn't instantly
//! kill the local proxy. When the control plane isn't configured, validation falls
//! back to the local key store (dev / transparent mode) via the caller.
use altkey_api::dto::{KeyValidateRequest, KeyValidateResponse};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct ControlPlaneValidator {
    base_url: String,
    agent_token: String,
    http: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, bool)>>,
    ttl: Duration,
    grace: Duration,
    last_ok: Mutex<Option<Instant>>,
}

impl ControlPlaneValidator {
    pub fn new(base_url: String, agent_token: String) -> Self {
        Self {
            base_url,
            agent_token,
            http: reqwest::Client::new(),
            cache: Mutex::new(HashMap::new()),
            ttl: Duration::from_secs(60),
            grace: Duration::from_secs(72 * 3600),
            last_ok: Mutex::new(None),
        }
    }

    /// True if the key is valid AND the subscription is active. Cached for `ttl`.
    /// On a network error, serve the last cached verdict within `grace`, else fail closed.
    pub async fn validate(&self, key: &str) -> bool {
        if let Some((at, ok)) = self.cache.lock().unwrap().get(key).copied() {
            if at.elapsed() < self.ttl {
                return ok;
            }
        }
        let resp = self
            .http
            .post(format!("{}/internal/key/validate", self.base_url))
            .json(&KeyValidateRequest {
                key: key.to_string(),
                agent_token: self.agent_token.clone(),
            })
            .send()
            .await;
        match resp {
            Ok(r) => match r.json::<KeyValidateResponse>().await {
                Ok(v) => {
                    let ok = v.valid && v.sub_active;
                    self.cache
                        .lock()
                        .unwrap()
                        .insert(key.to_string(), (Instant::now(), ok));
                    *self.last_ok.lock().unwrap() = Some(Instant::now());
                    ok
                }
                Err(_) => false,
            },
            Err(_) => {
                // Offline grace: if we succeeded recently, serve the last cached verdict.
                let within_grace = self
                    .last_ok
                    .lock()
                    .unwrap()
                    .map(|t| t.elapsed() < self.grace)
                    .unwrap_or(false);
                if within_grace {
                    self.cache
                        .lock()
                        .unwrap()
                        .get(key)
                        .map(|(_, ok)| *ok)
                        .unwrap_or(false)
                } else {
                    false // fail closed past the grace window
                }
            }
        }
    }
}

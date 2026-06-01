//! Best-effort usage reporter. Buffers `UsageRecordDto` records in memory and
//! POSTs them as a `UsageBatch` to the control-plane `/internal/usage` endpoint
//! on a background interval (every ~10 s). All operations are non-blocking:
//! `record` is a mutex-guarded push; `flush` drains the buffer and fires a
//! single HTTP request — on any error the batch is dropped and a warn is logged.
//!
//! The global `REPORTER` `OnceLock` is initialised in `main.rs` when both
//! `CONTROL_PLANE_URL` and `ALTKEY_AGENT_TOKEN` are set. When they are not set
//! the lock is never populated and `global_record(dto)` is a no-op.

use std::sync::{Mutex, OnceLock};
use std::sync::Arc;

use altkey_api::dto::{UsageBatch, UsageRecordDto};

/// Global singleton, populated by main.rs when control-plane is configured.
pub static REPORTER: OnceLock<Arc<UsageReporter>> = OnceLock::new();

/// Push one usage record via the global reporter (no-op if not configured).
pub fn global_record(dto: UsageRecordDto) {
    if let Some(r) = REPORTER.get() {
        r.record(dto);
    }
}

// ---------------------------------------------------------------------------

pub struct UsageReporter {
    base_url: String,
    agent_token: String,
    buffer: Mutex<Vec<UsageRecordDto>>,
    client: reqwest::Client,
}

impl UsageReporter {
    pub fn new(base_url: String, agent_token: String) -> Self {
        Self {
            base_url,
            agent_token,
            buffer: Mutex::new(Vec::new()),
            client: reqwest::Client::new(),
        }
    }

    /// Push a record into the in-memory buffer. Non-blocking (mutex-guarded sync
    /// push; the lock is never held while doing I/O).
    pub fn record(&self, dto: UsageRecordDto) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(dto);
        }
    }

    /// Drain the buffer and POST a `UsageBatch` to `{base_url}/internal/usage`.
    /// Best-effort: any HTTP error drops the batch silently (warn logged). Never
    /// blocks the caller beyond the time to acquire the buffer lock.
    pub async fn flush(&self) {
        // Drain the buffer under the lock, release immediately before doing I/O.
        let records: Vec<UsageRecordDto> = {
            match self.buffer.lock() {
                Ok(mut buf) => {
                    if buf.is_empty() {
                        return;
                    }
                    std::mem::take(&mut *buf)
                }
                Err(_) => return,
            }
        };

        let batch = UsageBatch {
            agent_token: self.agent_token.clone(),
            records,
        };

        let url = format!("{}/internal/usage", self.base_url.trim_end_matches('/'));
        match self.client.post(&url).json(&batch).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("usage: flushed {} records", batch.records.len());
            }
            Ok(resp) => {
                tracing::warn!(
                    "usage: control-plane returned {} — dropping batch of {} records",
                    resp.status(),
                    batch.records.len()
                );
            }
            Err(e) => {
                tracing::warn!(
                    "usage: flush failed ({}) — dropping batch of {} records (best-effort)",
                    e,
                    batch.records.len()
                );
            }
        }
    }
}

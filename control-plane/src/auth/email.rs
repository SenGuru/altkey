//! Pluggable email delivery for magic links. Prod wires a real sender (Resend/SMTP);
//! dev logs the link; tests capture it. The trait keeps handlers testable offline.
use anyhow::Result;
use std::sync::{Arc, Mutex};

#[async_trait::async_trait]
pub trait EmailSender: Send + Sync {
    async fn send_magic_link(&self, to: &str, link: &str) -> Result<()>;
}

/// Dev/default: log the magic link instead of sending it.
pub struct LoggingEmailSender;

#[async_trait::async_trait]
impl EmailSender for LoggingEmailSender {
    async fn send_magic_link(&self, to: &str, link: &str) -> Result<()> {
        tracing::info!("magic link for {to}: {link}");
        Ok(())
    }
}

/// Test sender: captures (to, link) pairs for assertions.
#[derive(Clone, Default)]
pub struct CapturingEmailSender {
    pub sent: Arc<Mutex<Vec<(String, String)>>>,
}

#[async_trait::async_trait]
impl EmailSender for CapturingEmailSender {
    async fn send_magic_link(&self, to: &str, link: &str) -> Result<()> {
        self.sent.lock().unwrap().push((to.to_string(), link.to_string()));
        Ok(())
    }
}

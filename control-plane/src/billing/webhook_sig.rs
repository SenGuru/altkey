//! Standard-Webhooks (svix-style) signature verification, as Polar uses. The signed
//! payload is `{id}.{timestamp}.{body}`; the secret is base64 (optionally `whsec_`-
//! prefixed); the `webhook-signature` header holds space-separated `v1,<b64sig>`.
use anyhow::{anyhow, Result};
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub struct WebhookHeaders {
    pub id: String,
    pub timestamp: String,
    pub signature: String, // raw header value (may contain multiple "v1,..." entries)
}

/// Verify the signature over the raw body. Returns Ok(()) if any provided v1 sig matches.
pub fn verify(secret: &str, headers: &WebhookHeaders, body: &[u8]) -> Result<()> {
    let key_b64 = secret.strip_prefix("whsec_").unwrap_or(secret);
    let key = base64::engine::general_purpose::STANDARD
        .decode(key_b64)
        .map_err(|_| anyhow!("webhook secret not base64"))?;

    let signed = format!(
        "{}.{}.{}",
        headers.id,
        headers.timestamp,
        String::from_utf8_lossy(body)
    );
    let mut mac = HmacSha256::new_from_slice(&key).map_err(|_| anyhow!("bad hmac key"))?;
    mac.update(signed.as_bytes());
    let expected = mac.finalize().into_bytes();

    for part in headers.signature.split(' ') {
        // Each part is "v1,<base64sig>"; take the portion after the comma.
        let sig_b64 = part.split_once(',').map(|(_, s)| s).unwrap_or(part);
        if let Ok(provided) = base64::engine::general_purpose::STANDARD.decode(sig_b64) {
            if provided.len() == expected.len() && provided.ct_eq(&*expected).into() {
                return Ok(());
            }
        }
    }
    Err(anyhow!("no matching webhook signature"))
}

/// Compute the header signature for a body — used by tests (and never in prod).
pub fn sign(secret: &str, id: &str, timestamp: &str, body: &[u8]) -> String {
    let key_b64 = secret.strip_prefix("whsec_").unwrap_or(secret);
    let key = base64::engine::general_purpose::STANDARD
        .decode(key_b64)
        .unwrap();
    let signed = format!("{}.{}.{}", id, timestamp, String::from_utf8_lossy(body));
    let mut mac = HmacSha256::new_from_slice(&key).unwrap();
    mac.update(signed.as_bytes());
    let sig =
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    format!("v1,{sig}")
}

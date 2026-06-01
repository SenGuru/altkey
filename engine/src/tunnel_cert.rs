//! Certificate the agent presents for `<handle>.altkey.app` when terminating
//! the public TLS. Trait so a real ACME DNS-01 impl can replace the self-signed
//! one later (Plan 2 ships only the self-signed impl; the integration test trusts it).
use anyhow::Result;
use rcgen::{CertificateParams, DnType, KeyPair};

pub trait HandleCert: Send + Sync {
    /// Returns (cert_chain_pem, private_key_pem) for `host` (e.g. "sen.altkey.app").
    fn cert_for(&self, host: &str) -> Result<(String, String)>;
}

/// Self-signed cert per host. Fine for local/loopback + tests; NOT publicly
/// trusted (a real client must opt to trust it). Production uses ACME instead.
pub struct SelfSignedHandleCert;

impl HandleCert for SelfSignedHandleCert {
    fn cert_for(&self, host: &str) -> Result<(String, String)> {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec![host.to_string()])?;
        params.distinguished_name.push(DnType::CommonName, host);
        let cert = params.self_signed(&key)?;
        Ok((cert.pem(), key.serialize_pem()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_signed_emits_cert_and_key_for_host() {
        let (cert, key) = SelfSignedHandleCert.cert_for("sen.altkey.app").unwrap();
        assert!(cert.contains("BEGIN CERTIFICATE"));
        assert!(key.contains("PRIVATE KEY"));
    }
}

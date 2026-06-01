//! Local root CA + per-host leaf certificate generation for transparent mode.
//! The CA is generated once and stored at ~/.altkey/ca.{crt,key}. Leaf certs for
//! intercepted hosts (api.openai.com etc.) are minted on demand, signed by the CA.
use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use std::path::Path;

/// An in-memory CA: the self-signed cert PEM + its key pair.
pub struct Ca {
    pub cert_pem: String,
    key: KeyPair,
    cert: rcgen::Certificate,
}

/// A minted leaf: cert chain PEM (leaf only) + private key PEM.
pub struct Leaf {
    pub cert_pem: String,
    pub key_pem: String,
}

impl Ca {
    pub fn generate() -> Result<Ca> {
        let key = KeyPair::generate().context("ca keypair")?;
        let mut params = CertificateParams::new(Vec::<String>::new()).context("ca params")?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        params.distinguished_name.push(DnType::CommonName, "altkey local CA");
        params.distinguished_name.push(DnType::OrganizationName, "altkey");
        let cert = params.self_signed(&key).context("self-sign ca")?;
        Ok(Ca { cert_pem: cert.pem(), key, cert })
    }

    pub fn mint_leaf(&self, host: &str) -> Result<Leaf> {
        let leaf_key = KeyPair::generate().context("leaf keypair")?;
        let mut params = CertificateParams::new(vec![host.to_string()]).context("leaf params")?;
        params.distinguished_name.push(DnType::CommonName, host);
        let leaf = params.signed_by(&leaf_key, &self.cert, &self.key).context("sign leaf")?;
        Ok(Leaf { cert_pem: leaf.pem(), key_pem: leaf_key.serialize_pem() })
    }

    pub fn save(&self, cert_path: &Path, key_path: &Path) -> Result<()> {
        std::fs::write(cert_path, &self.cert_pem).context("write ca cert")?;
        std::fs::write(key_path, self.key.serialize_pem()).context("write ca key")?;
        Ok(())
    }

    pub fn load_or_create(cert_path: &Path, key_path: &Path) -> Result<Ca> {
        if cert_path.exists() && key_path.exists() {
            let key_pem = std::fs::read_to_string(key_path).context("read ca key")?;
            let key = KeyPair::from_pem(&key_pem).context("parse ca key")?;
            let cert_pem = std::fs::read_to_string(cert_path).context("read ca cert")?;
            let params = CertificateParams::from_ca_cert_pem(&cert_pem).context("parse ca cert")?;
            let cert = params.self_signed(&key).context("rebuild ca")?;
            Ok(Ca { cert_pem, key, cert })
        } else {
            let ca = Ca::generate()?;
            ca.save(cert_path, key_path)?;
            Ok(ca)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_generates_and_mints_leaf_with_san() {
        let ca = Ca::generate().unwrap();
        assert!(ca.cert_pem.contains("BEGIN CERTIFICATE"));
        let leaf = ca.mint_leaf("api.openai.com").unwrap();
        assert!(leaf.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(leaf.key_pem.contains("PRIVATE KEY"));
        let parsed = CertificateParams::from_ca_cert_pem(&leaf.cert_pem);
        assert!(parsed.is_ok(), "leaf cert should parse");
    }

    #[test]
    fn ca_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("altkey-ca-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cert_p = dir.join("ca.crt");
        let key_p = dir.join("ca.key");
        let ca1 = Ca::load_or_create(&cert_p, &key_p).unwrap();
        let ca2 = Ca::load_or_create(&cert_p, &key_p).unwrap();
        assert_eq!(ca1.cert_pem, ca2.cert_pem);
        assert!(ca2.mint_leaf("api.anthropic.com").is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }
}

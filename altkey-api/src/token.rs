//! Token formats for altkey-cloud. Two kinds, both `<prefix><random>`:
//! - `ak_agent_…` identifies one paired machine to the cloud (relay + validation API).
//! - `ak_live_…`  is the endpoint key a calling app sends; the agent validates it.
//! Secrets are shown to the user exactly once; the cloud stores only `hash()`.
use rand::Rng;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Agent,
    Live,
}

impl TokenKind {
    pub fn prefix_str(self) -> &'static str {
        match self {
            TokenKind::Agent => "ak_agent_",
            TokenKind::Live => "ak_live_",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    /// The full plaintext token including the kind prefix. Show once, never stored.
    pub secret: String,
}

/// 40 random lowercase-alnum chars with the kind prefix.
pub fn generate(kind: TokenKind) -> Token {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let body: String = (0..40)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    Token { kind, secret: format!("{}{}", kind.prefix_str(), body) }
}

/// A short, safe-to-display prefix of a token (first 15 chars).
pub fn prefix(token: &str) -> String {
    token.chars().take(15).collect()
}

/// SHA-256 hex digest — what gets stored at rest.
pub fn hash(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex::encode(h.finalize())
}

/// Classify a presented token by its prefix, if recognized.
pub fn kind_of(token: &str) -> Option<TokenKind> {
    if token.starts_with(TokenKind::Agent.prefix_str()) {
        Some(TokenKind::Agent)
    } else if token.starts_with(TokenKind::Live.prefix_str()) {
        Some(TokenKind::Live)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_has_prefix_and_is_unique() {
        let a = generate(TokenKind::Agent);
        let b = generate(TokenKind::Agent);
        assert!(a.secret.starts_with("ak_agent_"));
        assert_eq!(a.kind, TokenKind::Agent);
        assert_ne!(a.secret, b.secret, "tokens must be random");
        assert!(generate(TokenKind::Live).secret.starts_with("ak_live_"));
    }

    #[test]
    fn hash_is_stable_and_prefix_is_short() {
        let t = generate(TokenKind::Live);
        assert_eq!(hash(&t.secret), hash(&t.secret), "hash is deterministic");
        assert_ne!(hash(&t.secret), t.secret, "hash != plaintext");
        assert_eq!(hash(&t.secret).len(), 64, "sha256 hex is 64 chars");
        assert_eq!(prefix(&t.secret), &t.secret[..15]);
    }

    #[test]
    fn kind_of_classifies() {
        assert_eq!(kind_of("ak_agent_xyz"), Some(TokenKind::Agent));
        assert_eq!(kind_of("ak_live_xyz"), Some(TokenKind::Live));
        assert_eq!(kind_of("nope"), None);
    }
}

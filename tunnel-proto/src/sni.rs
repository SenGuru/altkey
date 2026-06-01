//! Read the leading TLS ClientHello from a stream and extract the SNI server
//! name WITHOUT decrypting anything (SNI is plaintext in the ClientHello). The
//! bytes consumed are returned so the caller can replay them downstream.
use anyhow::{bail, Result};
use tokio::io::{AsyncRead, AsyncReadExt};

/// Peek the SNI host name out of the first TLS record on `stream`.
///
/// Reads exactly the first TLS record (5-byte record header + record body),
/// which for a fresh connection is the ClientHello. Parses the plaintext SNI
/// `host_name` extension and returns `(sni, consumed_bytes)` where
/// `consumed_bytes` is precisely the bytes read off the stream so the caller
/// can replay them to the downstream agent.
pub async fn peek_sni<R: AsyncRead + Unpin>(stream: &mut R) -> Result<(String, Vec<u8>)> {
    let mut head = [0u8; 5];
    stream.read_exact(&mut head).await?;
    if head[0] != 0x16 {
        bail!("not a TLS handshake record (type=0x{:02x})", head[0]);
    }
    let rec_len = u16::from_be_bytes([head[3], head[4]]) as usize;
    if rec_len == 0 || rec_len > 16 * 1024 {
        bail!("implausible ClientHello length {rec_len}");
    }
    let mut rec = vec![0u8; rec_len];
    stream.read_exact(&mut rec).await?;
    let mut consumed = Vec::with_capacity(5 + rec_len);
    consumed.extend_from_slice(&head);
    consumed.extend_from_slice(&rec);
    let sni = parse_sni(&rec).ok_or_else(|| anyhow::anyhow!("no SNI in ClientHello"))?;
    Ok((sni, consumed))
}

/// Parse the SNI `host_name` out of a TLS handshake record body (the bytes
/// following the 5-byte record header). Returns `None` if the body is not a
/// ClientHello or carries no SNI extension.
fn parse_sni(handshake_body: &[u8]) -> Option<String> {
    use tls_parser::{
        parse_tls_message_handshake, TlsExtension, TlsMessage, TlsMessageHandshake,
    };
    let (_, msg) = parse_tls_message_handshake(handshake_body).ok()?;
    let ch = match msg {
        TlsMessage::Handshake(TlsMessageHandshake::ClientHello(ch)) => ch,
        _ => return None,
    };
    let ext_bytes = ch.ext?;
    let (_, exts) = tls_parser::parse_tls_extensions(ext_bytes).ok()?;
    for ext in exts {
        if let TlsExtension::SNI(names) = ext {
            for (_typ, name) in names {
                if let Ok(s) = std::str::from_utf8(name) {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client_hello_for(host: &str) -> Vec<u8> {
        use std::sync::Arc;
        // ring provider (matches the rest of the workspace).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let root = rustls::RootCertStore::empty();
        let cfg = rustls::ClientConfig::builder()
            .with_root_certificates(root)
            .with_no_client_auth();
        let server_name = rustls::pki_types::ServerName::try_from(host.to_string()).unwrap();
        let mut conn = rustls::ClientConnection::new(Arc::new(cfg), server_name).unwrap();
        let mut buf = Vec::new();
        // Drain all pending handshake bytes (the ClientHello first flight).
        while conn.wants_write() {
            if conn.write_tls(&mut buf).unwrap() == 0 {
                break;
            }
        }
        buf
    }

    #[tokio::test]
    async fn extracts_sni_from_client_hello() {
        let hello = client_hello_for("example.com");
        let mut cursor = std::io::Cursor::new(hello.clone());
        let (sni, consumed) = peek_sni(&mut cursor).await.unwrap();
        assert_eq!(sni, "example.com");
        // consumed must be a prefix of (or equal to) the produced bytes — it is
        // exactly the first record. Assert it's the leading bytes of `hello`.
        assert_eq!(
            &hello[..consumed.len()],
            &consumed[..],
            "consumed bytes must match the leading ClientHello record for replay"
        );
        assert!(!consumed.is_empty());
    }
}

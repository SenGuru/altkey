//! The Apple client-secret JWT must be ES256, carry the key id in the header, and
//! the standard iss/aud/sub claims. We sign with a throwaway EC P-256 key generated
//! at runtime — no static key is committed (no real Apple key is ever involved).
use control_plane::auth::oauth::apple::client_secret_jwt;

/// Generate a throwaway P-256 PKCS#8 PEM key at test time using the `p256` crate.
/// This is Option B from the task spec: runtime generation, reproducible, no
/// committed static key, definitely not a real Apple key.
fn generate_test_p256_pem() -> String {
    use p256::pkcs8::EncodePrivateKey;
    let secret_key = p256::SecretKey::random(&mut rand::thread_rng());
    secret_key
        .to_pkcs8_pem(pkcs8::LineEnding::LF)
        .expect("p256 PKCS#8 PEM serialisation")
        .to_string()
}

#[test]
fn client_secret_is_es256_with_kid() {
    // Generate a fresh throwaway key for every test run — not a real Apple key.
    let pem = generate_test_p256_pem();

    let jwt =
        client_secret_jwt("TEAMID", "KEYID", "com.altkey.service", &pem, 1_700_000_000)
            .expect("client_secret_jwt must succeed with a valid P-256 PKCS#8 PEM");

    // The JWT has three dot-separated parts: header.payload.signature
    let header_b64 = jwt.split('.').next().expect("JWT must have a header part");

    use base64::Engine;
    let header_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(header_b64)
        .expect("JWT header must be valid base64url");
    let header: serde_json::Value =
        serde_json::from_slice(&header_bytes).expect("JWT header must be valid JSON");

    assert_eq!(header["alg"], "ES256", "JWT algorithm must be ES256 (Apple requirement)");
    assert_eq!(header["kid"], "KEYID", "JWT kid must match the supplied key ID");
}

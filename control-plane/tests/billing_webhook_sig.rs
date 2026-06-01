//! A body signed with the shared secret verifies; tampering or wrong secret fails.
use base64::Engine;
use control_plane::billing::webhook_sig::{sign, verify, WebhookHeaders};

fn secret() -> String {
    // 32 random-ish bytes base64'd, with the whsec_ prefix Polar uses.
    format!(
        "whsec_{}",
        base64::engine::general_purpose::STANDARD.encode([7u8; 32])
    )
}

#[test]
fn valid_signature_verifies_and_tamper_fails() {
    let s = secret();
    let body = br#"{"type":"subscription.active","data":{}}"#;
    let sig = sign(&s, "msg_1", "1700000000", body);
    let h = WebhookHeaders {
        id: "msg_1".into(),
        timestamp: "1700000000".into(),
        signature: sig,
    };

    assert!(verify(&s, &h, body).is_ok());

    // Tampered body fails.
    let tampered = br#"{"type":"subscription.canceled","data":{}}"#;
    assert!(verify(&s, &h, tampered).is_err());

    // Wrong secret fails.
    let other = format!(
        "whsec_{}",
        base64::engine::general_purpose::STANDARD.encode([9u8; 32])
    );
    assert!(verify(&other, &h, body).is_err());
}

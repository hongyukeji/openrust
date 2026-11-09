//! 防止凭据跨用途替换、错误挑战水合与加密轮换后丢失原绑定。

use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::encryption::EncryptionConfig;

fn codec(active: &str, keys: &[(&str, u8)]) -> Result<GrantTokenCodec> {
    let keys = keys
        .iter()
        .map(|(name, value)| ((*name).to_owned(), STANDARD.encode([*value; 32])))
        .collect();
    Ok(GrantTokenCodec::new(Arc::new(ContextualEncrypter::new(
        EncryptionConfig::new(active, keys),
    )?)))
}

fn code() -> Result<CodePayload> {
    let challenge = CodeChallenge::from_request(Some(&"x".repeat(43)), Some("plain"), true)
        .map_err(|_| invalid_envelope())?;
    let record = crate::grant_records::CodeRecord {
        client_id: "client-uuid".into(),
        id: "a".repeat(80),
        scopes: vec!["profile".into(), "email".into()],
        user_id: "user-string-id".into(),
        expires_at: 2_000_000_000,
        authority: GrantAuthorityBinding::new("test-verified-context".into())?,
    };
    Ok(CodePayload::new(&record, None, challenge.as_ref()))
}

#[test]
fn code_roundtrip_preserves_optional_redirect_and_validated_pkce() -> Result<()> {
    let codec = codec("current", &[("current", 7)])?;
    let original = code()?;
    let envelope = codec.encode_code(&original)?;
    assert!(!envelope.contains("user-string-id"));
    let restored = codec.decode_code(&envelope)?;
    assert_eq!(restored.client_id, original.client_id);
    assert_eq!(restored.user_id, original.user_id);
    assert_eq!(restored.redirect_uri, None);
    assert_eq!(restored.scopes, original.scopes);
    assert_eq!(restored.expire_time, original.expire_time);
    let challenge = restored.challenge()?.ok_or_else(invalid_envelope)?;
    assert!(challenge.validate_verifier(Some(&"x".repeat(43))).is_ok());
    assert!(challenge.validate_verifier(Some(&"y".repeat(43))).is_err());
    Ok(())
}

#[test]
fn code_and_refresh_cannot_be_substituted_even_with_same_application_key() -> Result<()> {
    let codec = codec("current", &[("current", 7)])?;
    let code = codec.encode_code(&code()?)?;
    let refresh = codec.encode_refresh(&RefreshPayload {
        client_id: "client-uuid".into(),
        refresh_token_id: "b".repeat(80),
        access_token_id: "c".repeat(80),
        scopes: vec!["profile".into()],
        user_id: "user-string-id".into(),
        expire_time: 2_000_000_000,
        authority_binding: "test-verified-context".into(),
    })?;
    assert!(codec.decode_refresh(&code).is_err());
    assert!(codec.decode_code(&refresh).is_err());
    assert_eq!(
        codec.decode_refresh(&refresh)?.access_token_id,
        "c".repeat(80)
    );
    Ok(())
}

#[test]
fn rotated_key_decrypts_old_code_only_while_old_key_is_configured() -> Result<()> {
    let original = codec("old", &[("old", 7)])?.encode_code(&code()?)?;
    assert!(
        codec("new", &[("old", 7), ("new", 9)])?
            .decode_code(&original)
            .is_ok()
    );
    assert!(codec("new", &[("new", 9)])?.decode_code(&original).is_err());
    let mut tampered = original.into_bytes();
    if let Some(last) = tampered.last_mut() {
        *last = if *last == b'A' { b'B' } else { b'A' };
    }
    let tampered = String::from_utf8(tampered).map_err(|_| invalid_envelope())?;
    assert!(codec("old", &[("old", 7)])?.decode_code(&tampered).is_err());
    Ok(())
}

#[test]
fn authenticated_payload_still_rejects_invalid_challenge_pair_and_identifier() -> Result<()> {
    let codec = codec("current", &[("current", 7)])?;
    let mut payload = code()?;
    payload.code_challenge_method = Some("unknown".into());
    let token = codec.encode_code(&payload)?;
    assert!(codec.decode_code(&token).is_err());
    payload.code_challenge_method = None;
    assert!(codec.decode_code(&codec.encode_code(&payload)?).is_err());
    payload.code_challenge = None;
    payload.auth_code_id = "short".into();
    assert!(codec.decode_code(&codec.encode_code(&payload)?).is_err());
    Ok(())
}

#[test]
fn random_ids_and_secrets_have_the_official_shapes() -> Result<()> {
    let first = random_identifier()?;
    let second = random_identifier()?;
    assert_ne!(first, second);
    assert_eq!(first.len(), 80);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let secret = generate_client_secret()?;
    assert_eq!(secret.len(), 40);
    assert!(secret.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    Ok(())
}

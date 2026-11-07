//! 固定 League 9.4.1 tests/Stubs RSA 测试密钥；不得用于部署。
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::test_keys::{OTHER_PUBLIC, PRIVATE, PUBLIC};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn signer() -> Result<OAuthAccessSigner, AccessSigningError> {
    OAuthAccessSigner::from_pem(PRIVATE)
}

fn verifier() -> Result<OAuthAccessVerifier, AccessSigningError> {
    OAuthAccessVerifier::from_pem(PUBLIC)
}

#[test]
fn preserves_original_subject_and_upstream_claim_shape() -> TestResult {
    let signer = signer()?;
    let verifier = verifier()?;
    let token = signer.sign_at(
        "client",
        "token-id",
        Some("0007-用户"),
        &["read".into()],
        100,
        200,
    )?;
    let payload = token.split('.').nth(1).ok_or("缺少 JWT 载荷")?;
    let raw: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?;
    assert_eq!(raw["aud"], "client");
    let claims = verifier.verify_at(&token, "client", 100)?;
    assert_eq!(claims.sub, "0007-用户");
    assert_eq!(claims.client_id(), Some("client"));
    assert_eq!(claims.jti, "token-id");
    assert_eq!(claims.scopes, ["read"]);
    assert_eq!((claims.iat, claims.nbf, claims.exp), (100, 100, 200));
    let json = serde_json::to_value(&claims)?;
    assert!(json.get("iss").is_none());
    assert!(json.get("client_id").is_none());
    let client_token = signer.sign_at("client", "client-token", None, &[], 100, 200)?;
    assert_eq!(
        verifier.verify_at(&client_token, "client", 100)?.sub,
        "client"
    );
    Ok(())
}

#[test]
fn signed_audience_arrays_keep_the_strict_single_client_boundary() -> TestResult {
    let signer = signer()?;
    let verifier = verifier()?;
    for (aud, valid) in [
        (serde_json::json!(["client"]), true),
        (serde_json::json!(["client", "client"]), false),
        (serde_json::json!(["client", "other"]), false),
        (serde_json::json!(["other"]), false),
        (serde_json::json!([]), false),
        (serde_json::json!(7), false),
    ] {
        let claims = serde_json::json!({
            "aud":aud, "jti":"token-id", "sub":"user", "scopes":[],
            "iat":100, "nbf":100, "exp":200,
        });
        let token = encode(&Header::new(Algorithm::RS256), &claims, &signer.signing)?;
        assert_eq!(verifier.verify_at(&token, "client", 100).is_ok(), valid);
    }
    Ok(())
}

#[test]
fn rejects_wrong_audience_and_exact_expiry_and_future_time() -> TestResult {
    let signer = signer()?;
    let verifier = verifier()?;
    let token = signer.sign_at("client", "token", Some("user"), &[], 100, 200)?;
    assert!(verifier.verify_at(&token, "other", 100).is_err());
    assert!(verifier.verify_at(&token, "client", 99).is_err());
    assert!(verifier.verify_at(&token, "client", 199).is_ok());
    assert!(verifier.verify_at(&token, "client", 200).is_err());
    assert!(
        signer
            .sign_at("client", "token", Some(""), &[], 100, 200)
            .is_err()
    );
    assert!(
        signer
            .sign_at("client", "token", None, &[], 100, 100)
            .is_err()
    );
    Ok(())
}

#[test]
fn rejects_wrong_key_and_redacts_debug_and_errors() -> TestResult {
    assert!(matches!(
        validate_key_pair(PRIVATE, OTHER_PUBLIC),
        Err(AccessSigningError::InvalidKeyPair)
    ));
    assert!(matches!(
        OAuthAccessSigner::from_pem(b"invalid"),
        Err(AccessSigningError::InvalidPrivateKey)
    ));
    let signer = signer()?;
    let token = signer.sign_at("client", "token", None, &[], 100, 200)?;
    let wrong = OAuthAccessVerifier::from_pem(OTHER_PUBLIC)?;
    assert!(wrong.verify_at(&token, "client", 100).is_err());
    assert!(!format!("{signer:?}").contains("PRIVATE"));
    assert_eq!(
        AccessSigningError::InvalidKeyPair.to_string(),
        "OAuth RSA signing key pair is invalid"
    );
    Ok(())
}

#[test]
fn rejects_algorithm_confusion_and_id_token_shape() -> TestResult {
    let signer = signer()?;
    let verifier = verifier()?;
    let claims = PassportAccessClaims {
        aud: vec!["client".into()],
        jti: "id".into(),
        sub: "user".into(),
        scopes: vec![],
        iat: 100,
        nbf: 100,
        exp: 200,
    };
    let forged = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(PUBLIC),
    )?;
    assert!(verifier.verify_at(&forged, "client", 100).is_err());
    let id_claims = serde_json::json!({ "iss": "issuer", "aud": ["client"], "sub": "user", "iat": 100, "nbf": 100, "exp": 200, "nonce": "nonce" });
    let id_token = encode(&Header::new(Algorithm::RS256), &id_claims, &signer.signing)?;
    assert!(verifier.verify_at(&id_token, "client", 100).is_err());
    Ok(())
}

#[test]
fn rejects_signed_malformed_access_claims() -> TestResult {
    let signer = signer()?;
    let verifier = verifier()?;
    for (issued, not_before, expires) in [(100, 150, 200), (150, 100, 200), (100, 100, 100)] {
        let claims = PassportAccessClaims {
            aud: vec!["client".into()],
            jti: "id".into(),
            sub: "user".into(),
            scopes: vec![],
            iat: issued,
            nbf: not_before,
            exp: expires,
        };
        let token = encode(&Header::new(Algorithm::RS256), &claims, &signer.signing)?;
        assert!(verifier.verify_at(&token, "client", 100).is_err());
    }
    let claims = PassportAccessClaims {
        aud: vec!["client".into(), "other".into()],
        jti: "id".into(),
        sub: "user".into(),
        scopes: vec![],
        iat: 100,
        nbf: 100,
        exp: 200,
    };
    let token = encode(&Header::new(Algorithm::RS256), &claims, &signer.signing)?;
    assert!(verifier.verify_at(&token, "client", 100).is_err());
    Ok(())
}

#[test]
fn independent_capabilities_do_not_require_a_matching_counterpart() -> TestResult {
    let signer = OAuthAccessSigner::from_pem(PRIVATE)?;
    let verifier = OAuthAccessVerifier::from_pem(PUBLIC)?;
    // 不配对的合法公钥也能独立构造；真实验证时拒绝此私钥签发的 token。
    let other_verifier = OAuthAccessVerifier::from_pem(OTHER_PUBLIC)?;
    let token = signer.sign_at("client", "private-only", Some("raw-user"), &[], 100, 200)?;
    assert_eq!(verifier.verify_at(&token, "client", 100)?.sub, "raw-user");
    assert!(other_verifier.verify_at(&token, "client", 100).is_err());
    assert!(validate_key_pair(PRIVATE, PUBLIC).is_ok());
    assert_eq!(
        validate_key_pair(PRIVATE, OTHER_PUBLIC),
        Err(AccessSigningError::InvalidKeyPair)
    );
    assert_eq!(
        validate_key_pair(b"invalid", PUBLIC),
        Err(AccessSigningError::InvalidKeyPair)
    );
    assert_eq!(
        validate_key_pair(PRIVATE, b"invalid"),
        Err(AccessSigningError::InvalidKeyPair)
    );
    Ok(())
}

#[test]
fn constructors_reject_asn1_shapes_without_valid_rsa_material() -> TestResult {
    let private = b"-----BEGIN RSA PRIVATE KEY-----\nMAMCAQE=\n-----END RSA PRIVATE KEY-----\n";
    let public = b"-----BEGIN RSA PUBLIC KEY-----\nMAMCAQE=\n-----END RSA PUBLIC KEY-----\n";
    // DER 为只含一个 INTEGER 的非空 SEQUENCE。低层解析接受，实际 RSA 构造必须拒绝。
    let _parsed_private = EncodingKey::from_rsa_pem(private)?;
    let _parsed_public = DecodingKey::from_rsa_pem(public)?;
    assert!(matches!(
        OAuthAccessSigner::from_pem(private),
        Err(AccessSigningError::InvalidPrivateKey)
    ));
    assert!(matches!(
        OAuthAccessVerifier::from_pem(public),
        Err(AccessSigningError::InvalidPublicKey)
    ));
    assert_eq!(
        validate_key_pair(private, PUBLIC),
        Err(AccessSigningError::InvalidKeyPair)
    );
    Ok(())
}

#[test]
fn capabilities_and_errors_redact_keys_and_keep_rsa_roles() -> TestResult {
    let signer = signer()?;
    let verifier = verifier()?;
    assert!(OAuthAccessSigner::from_pem(PUBLIC).is_err());
    assert!(OAuthAccessVerifier::from_pem(PRIVATE).is_err());
    assert!(OAuthAccessVerifier::from_pem(b"invalid").is_err());
    for debug in [format!("{signer:?}"), format!("{verifier:?}")] {
        assert!(!debug.contains("BEGIN"));
        assert!(!debug.contains("MII"));
    }
    assert_eq!(
        AccessSigningError::InvalidPrivateKey.to_string(),
        "OAuth RSA private signing key is invalid"
    );
    assert_eq!(
        AccessSigningError::InvalidPublicKey.to_string(),
        "OAuth RSA public verification key is invalid"
    );
    for error in [
        AccessSigningError::InvalidPrivateKey,
        AccessSigningError::InvalidPublicKey,
        AccessSigningError::InvalidKeyPair,
    ] {
        assert!(!format!("{error:?} {error}").contains("BEGIN"));
    }
    Ok(())
}

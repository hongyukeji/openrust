//! 固定表达式、RFC7636 向量与敏感信息边界的纯协议回归。

use openrust_passport::{CodeChallenge, CodeChallengeMethod, PkceError, verify_optional_challenge};

const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

fn parsed(value: &str, method: Option<&str>) -> Result<CodeChallenge, PkceError> {
    CodeChallenge::from_request(Some(value), method, true)?.ok_or(PkceError::InternalValidation)
}

#[test]
fn public_requires_challenge_and_confidential_can_omit_it() {
    assert!(matches!(
        CodeChallenge::from_request(None, None, true),
        Err(PkceError::MissingChallenge)
    ));
    assert!(matches!(
        CodeChallenge::from_request(None, None, false),
        Ok(None)
    ));
    assert!(matches!(
        CodeChallenge::from_request(None, Some("ignored_without_challenge"), false),
        Ok(None)
    ));
}

#[test]
fn unknown_and_case_changed_methods_are_rejected_before_challenge_format() {
    for method in ["s256", "PLAIN", "S512", "S 256", "\u{000c}S256"] {
        assert!(matches!(
            CodeChallenge::from_request(Some("bad"), Some(method), true),
            Err(PkceError::InvalidMethod)
        ));
    }
}

#[test]
fn method_serialization_uses_exact_wire_identifiers() -> Result<(), serde_json::Error> {
    assert_eq!(
        serde_json::to_string(&CodeChallengeMethod::Plain)?,
        "\"plain\""
    );
    assert_eq!(
        serde_json::to_string(&CodeChallengeMethod::S256)?,
        "\"S256\""
    );
    assert_eq!(
        serde_json::from_str::<CodeChallengeMethod>("\"plain\"")?,
        CodeChallengeMethod::Plain
    );
    assert_eq!(
        serde_json::from_str::<CodeChallengeMethod>("\"S256\"")?,
        CodeChallengeMethod::S256
    );
    assert!(serde_json::from_str::<CodeChallengeMethod>("\"s256\"").is_err());
    Ok(())
}

#[test]
fn rfc7636_appendix_b_s256_vector_matches() -> Result<(), PkceError> {
    assert_eq!(
        parsed(CHALLENGE, Some("S256"))?.validate_verifier(Some(VERIFIER)),
        Ok(())
    );
    Ok(())
}

#[test]
fn plain_uses_exact_comparison_and_retains_valid_mismatch() -> Result<(), PkceError> {
    let challenge = parsed(VERIFIER, Some("plain"))?;
    assert_eq!(
        challenge.validate_verifier(Some(CHALLENGE)),
        Err(PkceError::Mismatch)
    );
    Ok(())
}

#[test]
fn s256_does_not_accept_plain_text_as_the_digest() -> Result<(), PkceError> {
    assert_eq!(
        parsed(VERIFIER, Some("S256"))?.validate_verifier(Some(VERIFIER)),
        Err(PkceError::Mismatch)
    );
    Ok(())
}

#[test]
fn challenge_bounds_follow_fixed_43_through_128_expression() -> Result<(), PkceError> {
    for size in [43, 128] {
        let value = "a".repeat(size);
        assert_eq!(
            parsed(&value, Some("plain"))?.validate_verifier(Some(&value)),
            Ok(())
        );
    }
    for size in [1, 42, 129] {
        assert!(matches!(
            CodeChallenge::from_request(Some(&"a".repeat(size)), None, false),
            Err(PkceError::InvalidChallenge)
        ));
    }
    Ok(())
}

#[test]
fn s256_creation_does_not_invent_exact_43_character_restriction() -> Result<(), PkceError> {
    for size in [44, 64, 128] {
        let value = "a".repeat(size);
        let challenge = parsed(&value, Some("S256"))?;
        assert_eq!(
            challenge.validate_verifier(Some(VERIFIER)),
            Err(PkceError::Mismatch)
        );
    }
    Ok(())
}

#[test]
fn verifier_missing_and_invalid_format_remain_distinct() -> Result<(), PkceError> {
    let challenge = parsed(CHALLENGE, Some("S256"))?;
    assert_eq!(
        challenge.validate_verifier(None),
        Err(PkceError::MissingVerifier)
    );
    for value in [
        "a".repeat(42),
        "a".repeat(129),
        "é".repeat(43),
        format!("{}=", "a".repeat(43)),
    ] {
        assert_eq!(
            challenge.validate_verifier(Some(&value)),
            Err(PkceError::InvalidVerifier)
        );
    }
    Ok(())
}

#[test]
fn unreserved_ascii_symbols_are_accepted_without_case_normalization() -> Result<(), PkceError> {
    let value = "-._~a0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    assert_eq!(parsed(value, None)?.validate_verifier(Some(value)), Ok(()));
    for suffix in ["=", "+", "/", "\u{000c}", "é"] {
        assert!(matches!(
            CodeChallenge::from_request(Some(&format!("{}{suffix}", "a".repeat(43))), None, true),
            Err(PkceError::InvalidChallenge)
        ));
    }
    Ok(())
}

#[test]
fn no_challenge_with_nonempty_verifier_is_a_downgrade_error() {
    assert_eq!(verify_optional_challenge(None, None), Ok(()));
    for value in ["bad", VERIFIER] {
        assert_eq!(
            verify_optional_challenge(None, Some(value)),
            Err(PkceError::UnexpectedVerifier)
        );
    }
}

#[test]
fn optional_challenge_delegates_missing_mismatch_and_success() -> Result<(), PkceError> {
    let challenge = parsed(CHALLENGE, Some("S256"))?;
    assert_eq!(
        verify_optional_challenge(Some(&challenge), None),
        Err(PkceError::MissingVerifier)
    );
    assert_eq!(
        verify_optional_challenge(Some(&challenge), Some(CHALLENGE)),
        Err(PkceError::Mismatch)
    );
    assert_eq!(
        verify_optional_challenge(Some(&challenge), Some(VERIFIER)),
        Ok(())
    );
    Ok(())
}

#[test]
fn form_feed_and_unicode_whitespace_are_not_php_trim_characters() -> Result<(), PkceError> {
    let challenge = parsed(VERIFIER, None)?;
    for character in ['\u{000c}', '\u{00a0}', '\u{0085}'] {
        let raw = format!("{character}{VERIFIER}{character}");
        assert!(matches!(
            CodeChallenge::from_request(Some(&raw), None, true),
            Err(PkceError::InvalidChallenge)
        ));
        assert_eq!(
            challenge.validate_verifier(Some(&raw)),
            Err(PkceError::InvalidVerifier)
        );
        assert_eq!(
            verify_optional_challenge(None, Some(&character.to_string())),
            Err(PkceError::UnexpectedVerifier)
        );
        assert!(matches!(
            CodeChallenge::from_request(Some(VERIFIER), Some(&format!("{character}plain")), true),
            Err(PkceError::InvalidMethod)
        ));
    }
    Ok(())
}

#[test]
fn debug_and_error_text_do_not_include_secret_inputs() -> Result<(), PkceError> {
    let challenge = parsed(CHALLENGE, Some("S256"))?;
    let debug = format!("{challenge:?}");
    assert!(debug.contains("S256"));
    assert!(!debug.contains(CHALLENGE));
    assert!(!debug.contains(VERIFIER));
    for error in [
        PkceError::MissingChallenge,
        PkceError::InvalidChallenge,
        PkceError::InvalidMethod,
        PkceError::MissingVerifier,
        PkceError::InvalidVerifier,
        PkceError::UnexpectedVerifier,
        PkceError::Mismatch,
        PkceError::UnsupportedStoredMethod,
        PkceError::InternalValidation,
    ] {
        let output = format!("{error:?}: {error}");
        assert!(!output.contains(CHALLENGE));
        assert!(!output.contains(VERIFIER));
    }
    Ok(())
}

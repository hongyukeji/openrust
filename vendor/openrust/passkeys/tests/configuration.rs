use openrust_passkeys::WebAuthn;

#[test]
fn relying_party_policy_rejects_insecure_ambiguous_and_cross_site_origins() {
    let valid = ["https://example.com", "https://login.example.com:8443"];
    assert!(WebAuthn::new("example.com", &valid.map(str::to_owned), 60000).is_ok());
    assert!(WebAuthn::new("localhost", &["http://localhost:1234".into()], 60000).is_ok());
    for origin in [
        "http://example.com",
        "https://example.com/",
        "https://example.com/path",
        "https://example.com?query",
        "https://example.com#fragment",
        "https://user@example.com",
        "https://example.com.evil.test",
        "https://other.test",
    ] {
        assert!(
            WebAuthn::new("example.com", &[origin.into()], 60000).is_err(),
            "{origin}"
        );
    }
    for timeout in [0, u64::from(u32::MAX) + 1] {
        assert!(WebAuthn::new("example.com", &["https://example.com".into()], timeout).is_err());
    }
    assert!(WebAuthn::new("example.com", &[], 60000).is_err());
}

#[test]
fn upstream_options_and_server_only_state_round_trip_preserve_policy() {
    let web = WebAuthn::new("example.com", &["https://example.com".into()], 60000).unwrap();
    let options = web
        .registration_options(&"a".repeat(64), "email@example.com", "Name", &[])
        .unwrap();
    let value = options.options();
    assert_eq!(
        value["pubKeyCredParams"],
        serde_json::json!([{"type":"public-key","alg":-7},{"type":"public-key","alg":-257}])
    );
    assert_eq!(
        value["authenticatorSelection"]["userVerification"],
        "required"
    );
    assert_eq!(value["authenticatorSelection"]["residentKey"], "required");
    assert_eq!(value["attestation"], "none");
    assert_eq!(value["timeout"], 60000);
    assert_eq!(value["rp"]["id"], "example.com");
    assert_ne!(
        value["challenge"],
        web.registration_options(&"a".repeat(64), "email@example.com", "Name", &[])
            .unwrap()
            .options()["challenge"]
    );
    let copy: openrust_passkeys::RegistrationOptions =
        serde_json::from_value(serde_json::to_value(&options).unwrap()).unwrap();
    assert_eq!(copy.options(), value);
    assert!(
        web.registration_options(&"a".repeat(65), "email", "Name", &[])
            .is_err()
    );
    let verification = web.verification_options(&[]).unwrap();
    assert_eq!(
        verification.options()["allowCredentials"],
        serde_json::json!([])
    );
    assert_eq!(verification.options()["userVerification"], "required");
}

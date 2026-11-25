use http::{HeaderMap, Method};
use openrust_webhook_client::{
    DefaultSignatureValidator, Error, SignatureValidator, StandardSignatureValidator,
    WebhookConfig, WebhookConfigRepository, WebhookRequest,
};
use serde_json::Value;

fn request(body: Vec<u8>, name: &str, signature: &str) -> WebhookRequest {
    let mut headers = HeaderMap::new();
    headers.insert(
        http::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
        signature.parse().unwrap(),
    );
    WebhookRequest::new(Method::POST, headers, body).unwrap()
}
#[test]
fn spatie_protocol_vectors_cover_original_bytes_and_rejection_boundaries() {
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("reference/signatures.json")).unwrap();
    let config = WebhookConfig::new("default", "fixture-signing-secret").unwrap();
    for vector in vectors {
        let body: Vec<u8> = serde_json::from_value(vector["body"].clone()).unwrap();
        let signature = vector["signature"].as_str().unwrap();
        let valid = request(body.clone(), "Signature", signature);
        assert!(DefaultSignatureValidator.is_valid(&valid, &config).unwrap());
        assert!(
            !DefaultSignatureValidator
                .is_valid(&valid, &WebhookConfig::new("other", "wrong").unwrap())
                .unwrap()
        );
        let mut changed = body.clone();
        changed.push(b' ');
        assert!(
            !DefaultSignatureValidator
                .is_valid(&request(changed, "signature", signature), &config)
                .unwrap()
        );
        for rejected in [
            signature.to_uppercase(),
            signature[..62].to_owned(),
            String::new(),
            "0".into(),
            "f".repeat(65),
            "invalid".into(),
        ] {
            assert!(
                !DefaultSignatureValidator
                    .is_valid(&request(body.clone(), "signature", &rejected), &config)
                    .unwrap()
            );
        }
        let mut headers = valid.headers().clone();
        headers.append("signature", signature.parse().unwrap());
        assert!(
            !DefaultSignatureValidator
                .is_valid(
                    &WebhookRequest::new(Method::POST, headers, body).unwrap(),
                    &config
                )
                .unwrap()
        );
    }
}
#[test]
fn missing_secret_fails_only_when_a_signature_is_present_and_keys_rotate_within_profile() {
    let empty = WebhookConfig::new("default", "").unwrap();
    let missing = WebhookRequest::new(Method::POST, HeaderMap::new(), vec![]).unwrap();
    assert!(
        !DefaultSignatureValidator
            .is_valid(&missing, &empty)
            .unwrap()
    );
    assert!(matches!(
        DefaultSignatureValidator.is_valid(&request(vec![], "signature", &"a".repeat(64)), &empty),
        Err(Error::InvalidConfig("signing_secret"))
    ));
    assert!(matches!(
        DefaultSignatureValidator.is_valid(
            &request(vec![], "signature", &"a".repeat(64)),
            &WebhookConfig::new("zero", "0").unwrap()
        ),
        Err(Error::InvalidConfig("signing_secret"))
    ));
    let mut opaque = HeaderMap::new();
    opaque.insert("signature", http::HeaderValue::from_bytes(&[0xff]).unwrap());
    let opaque = WebhookRequest::new(Method::POST, opaque, vec![]).unwrap();
    assert!(matches!(
        DefaultSignatureValidator.is_valid(&opaque, &empty),
        Err(Error::InvalidConfig("signing_secret"))
    ));
    assert!(
        !DefaultSignatureValidator
            .is_valid(&opaque, &WebhookConfig::new("valid", "secret").unwrap())
            .unwrap()
    );
    let signature = "8cb61d0741591f3345143aba04d4bc0cc7d3338af72dfc1e0e6457004b7f3214e";
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("reference/signatures.json")).unwrap();
    let old = request(
        vec![],
        "signature",
        vectors[0]["signature"].as_str().unwrap(),
    );
    let config = WebhookConfig::new("default", "new-secret")
        .unwrap()
        .with_verification_secrets(["fixture-signing-secret".into()])
        .unwrap();
    assert!(DefaultSignatureValidator.is_valid(&old, &config).unwrap());
    assert!(!format!("{config:?}").contains("new-secret"));
    assert!(!format!("{config:?}").contains("fixture-signing-secret"));
    assert!(
        !DefaultSignatureValidator
            .is_valid(&request(vec![], "signature", signature), &config)
            .unwrap()
    );
    assert!(
        WebhookConfig::new("default", "x")
            .unwrap()
            .with_verification_secrets(vec!["s".into(); 8])
            .is_err()
    );
    let custom = config.with_signature_header("X-Signature").unwrap();
    assert!(
        DefaultSignatureValidator
            .is_valid(
                &request(
                    vec![],
                    "x-signature",
                    vectors[0]["signature"].as_str().unwrap()
                ),
                &custom
            )
            .unwrap()
    );
}
#[test]
fn configuration_replacement_retains_owned_snapshots_and_is_application_local() {
    let a = WebhookConfigRepository::new();
    let b = WebhookConfigRepository::new();
    a.add_config(WebhookConfig::new("A", "old").unwrap());
    let old = a.get_config("A").unwrap().unwrap();
    a.add_config(
        WebhookConfig::new("A", "new")
            .unwrap()
            .with_revision("2")
            .unwrap(),
    );
    assert_eq!(old.signing_secret(), "old");
    assert_eq!(a.get_config("A").unwrap().unwrap().signing_secret(), "new");
    assert!(a.get_config("a").unwrap().is_none());
    assert!(b.get_config("A").unwrap().is_none());
    assert_eq!(old.revision(), "1");
    assert!(WebhookConfig::new("", "secret").is_err());
    assert!(
        WebhookConfig::new("test", "x")
            .unwrap()
            .with_signature_header("bad\nheader")
            .is_err()
    );
    assert!(
        WebhookConfig::new("test", "x")
            .unwrap()
            .with_revision("../../x")
            .is_err()
    );
}
fn standard(body: &[u8], signature: &str, timestamp: i64) -> WebhookRequest {
    let mut headers = HeaderMap::new();
    headers.insert(
        "webhook-id",
        "msg_27UH4WbU6Z5A5EzD8u03UvzRbpk".parse().unwrap(),
    );
    headers.insert("webhook-timestamp", timestamp.to_string().parse().unwrap());
    headers.insert("webhook-signature", signature.parse().unwrap());
    WebhookRequest::new(Method::POST, headers, body.to_vec()).unwrap()
}
#[test]
fn official_standard_vector_timestamp_multi_signature_and_extreme_inputs() {
    let config = WebhookConfig::new("standard", "whsec_C2FVsBQIhrscChlQIMV+b5sSYspob7oD").unwrap();
    let body = br#"{"email":"test@example.com","username":"test_user"}"#;
    let signature = "v1,tZ1I4/hDygAJgO5TYxiSd6Sd0kDW6hPenDe+bTa3Kkw=";
    let request = standard(body, signature, 1649367553);
    for now in [1649367553, 1649367253, 1649367853] {
        assert!(
            StandardSignatureValidator
                .is_valid_at(&request, &config, now)
                .unwrap()
        )
    }
    for now in [1649367252, 1649367854] {
        assert!(
            !StandardSignatureValidator
                .is_valid_at(&request, &config, now)
                .unwrap()
        )
    }
    assert!(
        StandardSignatureValidator
            .is_valid_at(
                &standard(
                    body,
                    &format!("v2,garbage malformed {signature}"),
                    1649367553
                ),
                &config,
                1649367553
            )
            .unwrap()
    );
    assert!(
        !StandardSignatureValidator
            .is_valid_at(&standard(body, "v1,", 1649367553), &config, 1649367553)
            .unwrap()
    );
    for timestamp in [i64::MIN, i64::MAX] {
        assert!(
            !StandardSignatureValidator
                .is_valid_at(&standard(body, signature, timestamp), &config, 1649367553)
                .unwrap()
        );
    }
    assert!(
        StandardSignatureValidator
            .is_valid_at(&standard(body, signature, i64::MAX), &config, i64::MAX)
            .is_err()
    );
    for name in ["webhook-id", "webhook-signature", "webhook-timestamp"] {
        let mut headers = request.headers().clone();
        headers.append(
            http::header::HeaderName::from_static(name),
            headers[name].clone(),
        );
        assert!(
            !StandardSignatureValidator
                .is_valid_at(
                    &WebhookRequest::new(Method::POST, headers, body.to_vec()).unwrap(),
                    &config,
                    1649367553
                )
                .unwrap()
        );
    }
    assert!(
        !StandardSignatureValidator
            .is_valid_at(
                &standard(b"changed", signature, 1649367553),
                &config,
                1649367553
            )
            .unwrap()
    );
}
#[test]
fn input_budgets_reject_oversized_bodies_and_headers() {
    let config = WebhookConfig::new("budget", "x")
        .unwrap()
        .with_max_body_bytes(3)
        .unwrap();
    assert!(matches!(
        DefaultSignatureValidator.is_valid(&request(b"four".to_vec(), "signature", "x"), &config),
        Err(Error::PayloadTooLarge)
    ));
    let mut headers = HeaderMap::new();
    headers.insert("large", "a".repeat(65536).parse().unwrap());
    assert!(matches!(
        WebhookRequest::new(Method::POST, headers, vec![]),
        Err(Error::HeadersTooLarge)
    ));
    assert!(
        WebhookConfig::new("budget", "x")
            .unwrap()
            .with_max_body_bytes(0)
            .is_err()
    );
}

#[test]
fn external_validator_profile_and_response_contracts_are_constructible_without_private_access() {
    use openrust_webhook_client::{
        DefaultRespondsTo, ProcessEverythingWebhookProfile, RespondsToWebhook, Result,
        WebhookProfile,
    };
    struct HeaderValidator;
    impl SignatureValidator for HeaderValidator {
        fn is_valid(&self, request: &WebhookRequest, _: &WebhookConfig) -> Result<bool> {
            Ok(request.headers().contains_key("x-external-proof"))
        }
    }
    struct TextProfile;
    impl WebhookProfile for TextProfile {
        fn should_process(&self, request: &WebhookRequest, _: &WebhookConfig) -> Result<bool> {
            Ok(request.body() == b"process")
        }
    }
    struct SupplierResponse;
    impl RespondsToWebhook for SupplierResponse {
        fn respond_to_valid_webhook(
            &self,
            _: &WebhookRequest,
            _: &WebhookConfig,
        ) -> Result<openrust::http::Response> {
            Ok(openrust::http::Response::new(
                openrust::http::StatusCode::ACCEPTED,
                "queued".into(),
            ))
        }
    }
    let config = WebhookConfig::new("custom", "").unwrap();
    let target = request(b"process".to_vec(), "x-external-proof", "known")
        .with_uri("/hooks/provider?event=one%2Ftwo".parse().unwrap());
    assert_eq!(
        target.uri().path_and_query().unwrap().as_str(),
        "/hooks/provider?event=one%2Ftwo"
    );
    let request = request(b"process".to_vec(), "x-external-proof", "known");
    assert!(HeaderValidator.is_valid(&request, &config).unwrap());
    assert!(TextProfile.should_process(&request, &config).unwrap());
    assert!(
        ProcessEverythingWebhookProfile
            .should_process(&request, &config)
            .unwrap()
    );
    assert_eq!(
        DefaultRespondsTo
            .respond_to_valid_webhook(&request, &config)
            .unwrap()
            .status(),
        openrust::http::StatusCode::OK
    );
    assert_eq!(
        SupplierResponse
            .respond_to_valid_webhook(&request, &config)
            .unwrap()
            .status(),
        openrust::http::StatusCode::ACCEPTED
    );
}

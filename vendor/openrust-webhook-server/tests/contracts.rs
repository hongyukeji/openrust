use openrust_webhook_server::{
    BackoffStrategy, DefaultSigner, Error, ExponentialBackoffStrategy, Signer, SigningContext,
    StandardWebhookSigner,
};
use serde_json::Value;
#[test]
fn default_signature_vectors_bind_only_original_body_and_support_custom_header() {
    assert_eq!(
        DefaultSigner::default().signature_header_name(),
        "Signature"
    );
    assert_eq!(
        DefaultSigner::new("X-Custom-Signature")
            .unwrap()
            .signature_header_name(),
        "X-Custom-Signature"
    );
    let vectors: Vec<Value> =
        serde_json::from_str(include_str!("reference/signatures.json")).unwrap();
    for vector in vectors {
        let body: Vec<u8> = serde_json::from_value(vector["body"].clone()).unwrap();
        let context = SigningContext {
            webhook_url: "https://example.test/one",
            body: &body,
            secret: "fixture-signing-secret",
            message_id: "m-1",
            timestamp: 1,
        };
        let signature = DefaultSigner::default()
            .calculate_signature(&context)
            .unwrap();
        assert_eq!(signature, vector["signature"].as_str().unwrap());
        let changed = SigningContext {
            webhook_url: "https://other.test/path",
            message_id: "m-2",
            timestamp: 2,
            ..context
        };
        assert_eq!(
            signature,
            DefaultSigner::default()
                .calculate_signature(&changed)
                .unwrap()
        );
        let headers = DefaultSigner::new("X-Custom-Signature")
            .unwrap()
            .headers(&context)
            .unwrap();
        assert_eq!(headers["x-custom-signature"], signature);
        assert!(!headers.contains_key("timestamp"));
    }
    assert!(DefaultSigner::new("bad\r\nheader").is_err());
}
#[test]
fn standard_signer_uses_official_vector_and_all_protocol_headers() {
    let body = br#"{"email":"test@example.com","username":"test_user"}"#;
    let context = SigningContext {
        webhook_url: "https://example.test",
        body,
        secret: "whsec_C2FVsBQIhrscChlQIMV+b5sSYspob7oD",
        message_id: "msg_27UH4WbU6Z5A5EzD8u03UvzRbpk",
        timestamp: 1649367553,
    };
    let headers = StandardWebhookSigner.headers(&context).unwrap();
    assert_eq!(
        headers["webhook-signature"],
        "v1,tZ1I4/hDygAJgO5TYxiSd6Sd0kDW6hPenDe+bTa3Kkw="
    );
    assert_eq!(headers["webhook-timestamp"], "1649367553");
    assert_eq!(headers["webhook-id"], context.message_id);
    standardwebhooks::Webhook::new(context.secret)
        .unwrap()
        .verify_with_timestamp(body, &headers, context.timestamp)
        .unwrap();
    let binary = SigningContext {
        body: &[0xff],
        ..context
    };
    assert!(matches!(
        StandardWebhookSigner.calculate_signature(&binary),
        Err(Error::InvalidPayload)
    ));
    let invalid = SigningContext {
        message_id: "bad\nheader",
        ..context
    };
    assert!(StandardWebhookSigner.headers(&invalid).is_err());
    for id in ["", " leading", "trailing ", "tab\tid", "中文"] {
        assert!(
            StandardWebhookSigner
                .headers(&SigningContext {
                    message_id: id,
                    ..context
                })
                .is_err()
        );
    }
    assert!(
        StandardWebhookSigner
            .headers(&SigningContext {
                message_id: &"x".repeat(257),
                ..context
            })
            .is_err()
    );
    let invalid = SigningContext {
        secret: "invalid-base64",
        ..context
    };
    assert!(StandardWebhookSigner.headers(&invalid).is_err());
}
#[test]
fn backoff_matches_fixed_baseline_including_zero_and_large_attempts() {
    for (attempt, seconds) in [
        (0, 1),
        (1, 10),
        (2, 100),
        (3, 1000),
        (4, 10000),
        (5, 100000),
        (u32::MAX, 100000),
    ] {
        assert_eq!(
            ExponentialBackoffStrategy.wait_in_seconds_after_attempt(attempt),
            seconds
        );
    }
}

#[test]
fn independent_consumers_can_extend_signer_and_backoff_without_internal_registry_changes() {
    struct NamedSigner;
    impl Signer for NamedSigner {
        fn signature_header_name(&self) -> &str {
            "x-supplier-signature"
        }
        fn calculate_signature(
            &self,
            context: &SigningContext<'_>,
        ) -> openrust_webhook_server::Result<String> {
            DefaultSigner::default().calculate_signature(context)
        }
    }
    struct Linear;
    impl BackoffStrategy for Linear {
        fn wait_in_seconds_after_attempt(&self, attempt: u32) -> u64 {
            u64::from(attempt) * 2
        }
    }
    let signer: &dyn Signer = &NamedSigner;
    let context = SigningContext {
        webhook_url: "https://example.test",
        body: b"payload",
        secret: "private",
        message_id: "one",
        timestamp: 42,
    };
    assert_eq!(
        signer.headers(&context).unwrap()["x-supplier-signature"],
        DefaultSigner::default()
            .calculate_signature(&context)
            .unwrap()
    );
    let backoff: &dyn BackoffStrategy = &Linear;
    assert_eq!(backoff.wait_in_seconds_after_attempt(3), 6);
}

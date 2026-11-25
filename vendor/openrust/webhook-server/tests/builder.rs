use std::net::SocketAddr;

use http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use openrust_webhook_server::{TargetPolicy, WebhookCall};
use serde_json::json;

#[test]
fn prepared_body_headers_and_config_are_stable() {
    let mut headers = HeaderMap::new();
    headers.append("x-detail", HeaderValue::from_static("first"));
    headers.append("x-detail", HeaderValue::from_static("second"));
    headers.insert("x-message-id", HeaderValue::from_static("caller-id"));
    let call = WebhookCall::from_configuration(&json!({
        "signature_header_name": "X-Signature",
        "timestamp_header_name": "X-Time",
        "webhook_id_header_name": "X-Message-Id",
        "headers": {"X-Detail": "previous-value", "Content-Type": "application/json"},
        "timeout_in_seconds": 9,
        "tries": 5,
        "retryable_status_codes": [429, 503],
        "respect_retry_after": true,
        "retry_with_jitter": true,
        "throw_exception_on_failure": true
    }))
    .unwrap()
    .url("https://example.com/callback")
    .uuid("stable-id")
    .payload(json!({"z": "中/🙂", "a": 2}))
    .with_headers(headers)
    .use_timestamp(true)
    .use_secret_reference("credential.1")
    .on_queue(Some("receipts".into()))
    .on_connection(Some("database".into()))
    .meta(json!({"account": 3}))
    .with_tags(vec!["tenant".into()])
    .use_job("custom-receipt");
    assert_eq!(call.get_uuid(), "stable-id");
    assert_eq!(call.get_url(), Some("https://example.com/callback"));
    let prepared = call.prepare(&TargetPolicy::default()).unwrap();
    assert_eq!(prepared.body(), br#"{"z":"\u4e2d\/\ud83d\ude42","a":2}"#);
    assert_eq!(prepared.signing_bytes(), prepared.body());
    assert_eq!(prepared.method(), Method::POST);
    let signed = prepared.headers_at(900, Some("something")).unwrap();
    let later = prepared.headers_at(901, Some("something")).unwrap();
    assert!(prepared.headers_at(902, None).is_err());
    assert_eq!(signed["x-message-id"], "caller-id");
    assert_eq!(signed["x-time"], "900");
    assert_eq!(later["x-time"], "901");
    assert_eq!(signed["x-signature"], later["x-signature"]);
    assert!(signed["x-signature"].is_sensitive());
    assert_eq!(signed["content-type"], "application/json");
    assert_eq!(
        signed
            .get_all("x-detail")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(prepared.queue(), Some("receipts"));
    assert_eq!(prepared.connection(), Some("database"));
    assert_eq!(prepared.job(), "custom-receipt");
    assert_eq!(prepared.meta(), &json!({"account": 3}));
    assert_eq!(prepared.tags(), ["tenant"]);
    assert_eq!(prepared.tries(), 5);
    assert_eq!(prepared.timeout().as_secs(), 9);
    assert!(prepared.retries_status(StatusCode::TOO_MANY_REQUESTS));
    assert!(!prepared.retries_status(StatusCode::BAD_REQUEST));
    assert!(!prepared.retries_status(StatusCode::OK));
    assert!(
        prepared.respects_retry_after() && prepared.uses_jitter() && prepared.throws_on_failure()
    );
    assert_eq!(prepared.backoff_seconds(2), 100);
}

#[test]
fn target_policy_rejects_nonpublic_destinations_and_mixed_answers() {
    let policy = TargetPolicy::default();
    let url = "https://example.test/callback";
    let public: SocketAddr = "93.184.216.34:443".parse().unwrap();
    assert!(
        policy
            .bind(url, vec![public, "[2606:4700::1111]:443".parse().unwrap()])
            .is_ok()
    );
    for blocked in [
        "127.0.0.1:443",
        "169.254.169.254:443",
        "10.0.0.1:443",
        "100.100.100.200:443",
        "192.168.1.1:443",
        "192.0.2.1:443",
        "198.19.1.1:443",
        "0.0.0.0:443",
        "[::1]:443",
        "[::ffff:127.0.0.1]:443",
        "[fc00::1]:443",
        "[fe80::1]:443",
        "[64:ff9b::a9fe:a9fe]:443",
        "[2002:7f00:1::]:443",
        "[2001:db8::1]:443",
    ] {
        assert!(
            policy
                .bind(url, vec![public, blocked.parse().unwrap()])
                .is_err(),
            "{blocked}"
        );
    }
    for url in [
        "http://example.test/",
        "https://user:secret@example.test/",
        "file:///local",
        "https://example.test/#fragment",
        "https://example.test:0/",
        "https://0x7f000001/",
        "https://169.254.169.254/",
        "https://[::ffff:127.0.0.1]/",
    ] {
        assert!(
            WebhookCall::new()
                .unwrap()
                .url(url)
                .do_not_sign()
                .prepare(&policy)
                .is_err(),
            "{url}"
        );
    }
    assert!(policy.bind(url, vec![public; 65]).is_err());
    assert!(policy.bind(url, vec![]).is_err());
    assert!(
        policy
            .bind(url, vec!["93.184.216.34:80".parse().unwrap()])
            .is_err()
    );
    let local = "http://internal.test:8080/receiver";
    let local_ip = "127.0.0.1:8080".parse().unwrap();
    let internal = TargetPolicy::trusted_endpoint(local, vec![local_ip]).unwrap();
    let binding = internal.bind(local, vec![local_ip]).unwrap();
    assert_eq!(binding.addresses(), [local_ip]);
    assert_eq!(binding.url().as_str(), local);
    assert!(
        internal
            .bind("http://internal.test:8080/admin", vec![local_ip])
            .is_err()
    );
    assert!(
        internal
            .bind("http://other.test:8080/", vec![local_ip])
            .is_err()
    );
    assert!(
        internal
            .bind(local, vec!["127.0.0.2:8080".parse().unwrap()])
            .is_err()
    );
}

#[test]
fn builder_rejects_invalid_material_before_preparation() {
    let policy = TargetPolicy::default();
    let build = || {
        WebhookCall::new()
            .unwrap()
            .url("https://example.test/callback")
    };
    assert!(build().prepare(&policy).is_err());
    assert!(build().use_secret("0").prepare(&policy).is_err());
    assert!(
        build()
            .do_not_sign()
            .payload(json!(1))
            .prepare(&policy)
            .is_err()
    );
    assert!(
        build()
            .do_not_sign()
            .use_http_verb(Method::CONNECT)
            .prepare(&policy)
            .is_err()
    );
    assert!(
        build()
            .do_not_sign()
            .use_http_verb(Method::GET)
            .send_raw_body(b"x".to_vec())
            .prepare(&policy)
            .is_err()
    );
    assert!(
        build()
            .do_not_sign()
            .send_raw_body(vec![0; 16 * 1024 * 1024 + 1])
            .prepare(&policy)
            .is_err()
    );
    assert!(
        build()
            .do_not_sign()
            .timeout_in_seconds(121)
            .prepare(&policy)
            .is_err()
    );
    let mut headers = HeaderMap::new();
    headers.insert(header::HOST, HeaderValue::from_static("override.test"));
    assert!(
        build()
            .do_not_sign()
            .with_headers(headers)
            .prepare(&policy)
            .is_err()
    );
    assert!(
        build()
            .use_secret("secret")
            .sign_using("unknown-signer")
            .prepare(&policy)
            .is_err()
    );
    assert!(
        build()
            .do_not_sign()
            .use_backoff_strategy("unknown-backoff")
            .prepare(&policy)
            .is_err()
    );
}

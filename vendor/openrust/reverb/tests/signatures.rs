use std::collections::BTreeMap;

use openrust_reverb::protocols::pusher::signatures::{
    sign_channel, sign_request, verify_channel, verify_request,
};

const SECRET: &[u8] = b"test-secret";
const REQUEST_SIGNATURE: &str = "1028fac9fde1aed3c920054687a0326a48fe6d97ce8cf14af33a56d5ac935fbd";
const CHANNEL_SIGNATURE: &str = "fe5bcb580d59f7cf010174ff21c176d423925ea85f1f4dd919b9a685388e16c7";

#[test]
fn signed_request_binds_method_application_path_query_and_raw_body() {
    let body = br#"{"name":"example","data":"{}","channels":["private-orders.42"]}"#;
    let path = "/apps/test/events";
    let mut query = BTreeMap::from([
        ("auth_key".into(), "test-key".into()),
        ("auth_timestamp".into(), "1700000000".into()),
        ("auth_version".into(), "1.0".into()),
    ]);
    assert_eq!(
        sign_request(SECRET, "POST", path, &query, body),
        REQUEST_SIGNATURE
    );
    query.insert("auth_signature".into(), REQUEST_SIGNATURE.into());
    assert!(verify_request(SECRET, "POST", path, &query, body));
    assert!(!verify_request(SECRET, "GET", path, &query, body));
    assert!(!verify_request(
        SECRET,
        "POST",
        "/apps/other/events",
        &query,
        body
    ));
    assert!(!verify_request(SECRET, "POST", path, &query, b"{}"));
    query.insert("auth_key".into(), "other-key".into());
    assert!(!verify_request(SECRET, "POST", path, &query, body));
}

#[test]
fn presence_signature_binds_membership_data_and_socket() {
    let socket = "123.456";
    let channel = "presence-orders.42";
    let member = r#"{"user_id":"42","user_info":{"name":"Ada"}}"#;
    assert_eq!(
        sign_channel(SECRET, socket, channel, Some(member)),
        CHANNEL_SIGNATURE
    );
    assert!(verify_channel(
        SECRET,
        socket,
        channel,
        Some(member),
        CHANNEL_SIGNATURE
    ));
    assert!(!verify_channel(
        SECRET,
        socket,
        channel,
        None,
        CHANNEL_SIGNATURE
    ));
    assert!(!verify_channel(
        SECRET,
        "123.457",
        channel,
        Some(member),
        CHANNEL_SIGNATURE
    ));
    assert!(!verify_channel(
        b"other-secret",
        socket,
        channel,
        Some(member),
        CHANNEL_SIGNATURE
    ));
    assert!(!verify_channel(
        SECRET,
        socket,
        channel,
        Some(member),
        "bad signature"
    ));
}

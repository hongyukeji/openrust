use openrust::http::{Body, HttpError, Request};
use serde_json::json;

fn request(uri: &str) -> Request {
    let (parts, body) = ::http::Request::builder()
        .uri(uri)
        .body(Body::empty())
        .unwrap()
        .into_parts();
    Request::from_parts(parts, body)
}

#[test]
fn query_uses_php_variable_registration_and_exact_key_lookup() {
    let request = request("/?a[]=x&a[]=y&b[name]=Ada&top.key=one&top.key=two&escaped=%E4%B8%AD");
    assert_eq!(request.query(Some("a")).unwrap(), Some(json!(["x", "y"])));
    assert_eq!(
        request.query(Some("b")).unwrap(),
        Some(json!({"name":"Ada"}))
    );
    assert_eq!(request.query(Some("b.name")).unwrap(), None);
    assert_eq!(request.query(Some("top_key")).unwrap(), Some(json!("two")));
    assert_eq!(request.query(Some("escaped")).unwrap(), Some(json!("中")));
}

#[test]
fn malformed_and_oversized_query_input_returns_typed_http_errors() {
    assert!(matches!(
        request("/?name=%FF").query(None),
        Err(HttpError::InvalidRequest)
    ));
    let uri = format!("/?name={}", "x".repeat(16 * 1024));
    assert!(matches!(
        request(&uri).query(None),
        Err(HttpError::PayloadTooLarge)
    ));
}

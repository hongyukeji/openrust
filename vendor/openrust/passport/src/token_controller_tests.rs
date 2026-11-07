//! 使用真实 Core Request 提取器验证正文来源与完整字段投影。

use super::*;
use openrust::http::axum::{body::Body, extract::FromRequest, http::Request as Wire};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn request(
    content_type: &str,
    body: &str,
    query: &str,
) -> Result<Request, Box<dyn std::error::Error>> {
    let wire = Wire::builder()
        .method("POST")
        .uri(format!("/oauth/token{query}"))
        .header("content-type", content_type)
        .body(Body::from(body.to_owned()))?;
    Ok(Request::from_request(wire, &()).await?)
}

#[tokio::test]
async fn json_uses_complete_body_without_query_fallback() -> TestResult {
    let request = request("application/json", r#"{"grant_type":"authorization_code","extension.key":"body","extension":{"nested":[1,2]},"nullable":null}"#,
        "?grant_type=refresh_token&query_only=injected").await?;
    let values = body_parameters(&request)?;
    assert_eq!(values.get("grant_type"), Some(&json!("authorization_code")));
    assert_eq!(values.get("extension.key"), Some(&json!("body")));
    assert_eq!(values.get("extension"), Some(&json!({"nested":[1,2]})));
    assert_eq!(values.get("nullable"), Some(&Value::Null));
    assert!(!values.contains_key("query_only"));
    Ok(())
}

#[tokio::test]
async fn json_top_level_array_keeps_numeric_string_keys() -> TestResult {
    let request = request(
        "application/json",
        r#"["first",{"custom":"second"}]"#,
        "?0=query",
    )
    .await?;
    assert_eq!(
        body_parameters(&request)?,
        Map::from_iter([
            ("0".into(), json!("first")),
            ("1".into(), json!({"custom":"second"})),
        ])
    );
    Ok(())
}

#[tokio::test]
async fn form_keeps_unknown_literal_keys_and_nested_values() -> TestResult {
    let request = request(
        "application/x-www-form-urlencoded",
        "grant_type=authorization_code&extension.key=body&custom[nested]=value&empty=",
        "?grant_type=refresh_token&query_only=injected&extension.key=query",
    )
    .await?;
    let values = body_parameters(&request)?;
    assert_eq!(values.get("grant_type"), Some(&json!("authorization_code")));
    // PHP 表单解析器把点号正规化为下划线；adapter 保留 Core 产出的直接键。
    assert_eq!(values.get("extension_key"), Some(&json!("body")));
    assert_eq!(values.get("custom"), Some(&json!({"nested":"value"})));
    assert!(!values.contains_key("query_only"));
    assert!(!values.contains_key("empty"));
    assert_eq!(
        OAuthParameters::new(values).parameter("empty", Some("fallback"))?,
        Some("fallback".into())
    );
    Ok(())
}

#[tokio::test]
async fn multipart_keeps_fields_without_file_or_query_values() -> TestResult {
    let body = concat!(
        "--fixture\r\nContent-Disposition: form-data; name=\"grant_type\"\r\n\r\nauthorization_code\r\n",
        "--fixture\r\nContent-Disposition: form-data; name=\"custom\"\r\n\r\nbody\r\n",
        "--fixture\r\nContent-Disposition: form-data; name=\"file_only\"; filename=\"fixture.txt\"\r\nContent-Type: text/plain\r\n\r\nnot-a-parameter\r\n",
        "--fixture--\r\n"
    );
    let request = request(
        "multipart/form-data; boundary=fixture",
        body,
        "?query_only=injected&custom=query",
    )
    .await?;
    let values = body_parameters(&request)?;
    assert_eq!(values.get("grant_type"), Some(&json!("authorization_code")));
    assert_eq!(values.get("custom"), Some(&json!("body")));
    assert!(!values.contains_key("query_only"));
    assert!(!values.contains_key("file_only"));
    Ok(())
}

#[tokio::test]
async fn non_body_query_does_not_supply_grant_parameters() -> TestResult {
    let request = request(
        "application/x-www-form-urlencoded",
        "",
        "?grant_type=authorization_code&client_secret=query",
    )
    .await?;
    assert!(body_parameters(&request)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn authorization_preserves_first_original_text_value() -> TestResult {
    let wire = Wire::builder()
        .method("POST")
        .uri("/oauth/token")
        .header("authorization", "Basic Zmlyc3Q6c2VjcmV0")
        .header("authorization", "Basic c2Vjb25kOnNlY3JldA==")
        .body(Body::empty())?;
    let request = Request::from_request(wire, &()).await?;
    assert_eq!(
        request.header("Authorization")?,
        Some("Basic Zmlyc3Q6c2VjcmV0")
    );
    Ok(())
}

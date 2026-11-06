//! 输入边界的真实Request解析回归；完整Guard/Web接线由组合HTTP门禁验收。

use super::*;
use axum::{
    body::Body,
    extract::FromRequest,
    http::{Request as HttpRequest, header},
};

async fn request(content_type: &str, body: &str) -> Result<Request> {
    let incoming = HttpRequest::post("/login")
        .header(header::HOST, "example.test")
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body.to_owned()))
        .map_err(Error::internal)?;
    Request::from_request(incoming, &()).await
}

#[tokio::test]
async fn json_and_form_use_the_same_normalized_request_inputs() -> Result<()> {
    for (content_type, body) in [
        (
            "application/json",
            r#"{"username":"  Alice  ","password":"secret","remember":"yes"}"#,
        ),
        (
            "application/x-www-form-urlencoded",
            "username=%20%20Alice%20%20&password=secret&remember=yes",
        ),
    ] {
        let request = request(content_type, body).await?;
        assert_eq!(
            string_input(&request, "username", true)?.as_deref(),
            Some("Alice")
        );
        assert_eq!(
            string_input(&request, "password", true)?.as_deref(),
            Some("secret")
        );
        assert!(boolean(request.input("remember")?));
    }
    Ok(())
}

#[tokio::test]
async fn required_strings_reject_arrays_numbers_and_empty_values() -> Result<()> {
    for value in [json!(null), json!(12), json!([]), json!({}), json!("")] {
        let request = request("application/json", &json!({"username":value}).to_string()).await?;
        assert!(matches!(
            string_input(&request, "username", true),
            Err(Error::Validation(_))
        ));
    }
    let request = request("application/json", "{}").await?;
    assert_eq!(string_input(&request, "code", false)?, None);
    Ok(())
}

#[test]
fn force_and_remember_follow_laravel_boolean_rules() {
    for value in [
        json!(true),
        json!(1),
        json!("1"),
        json!("TRUE"),
        json!(" on "),
        json!("yes"),
    ] {
        assert!(boolean(Some(value)));
    }
    for value in [
        json!(false),
        json!(2),
        json!("false"),
        json!("arbitrary"),
        json!([]),
        json!(null),
    ] {
        assert!(!boolean(Some(value)));
    }
    assert!(!boolean(None));
}

#[test]
fn password_status_seconds_preserve_php_scalar_integer_semantics() {
    for (input, expected) in [
        (json!(-5), -5),
        (json!("-12seconds"), -12),
        (json!("1.9"), 1),
        (json!("1e2suffix"), 100),
        (json!(true), 1),
        (json!(false), 0),
        (json!(null), 0),
        (json!("non-numeric"), 0),
        (json!("  +20"), 20),
    ] {
        assert_eq!(php_integer(&input), expected);
    }
}

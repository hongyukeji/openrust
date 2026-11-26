use openrust::http::{Body, HttpError, Method, Request};
use serde_json::{Value, json};

fn incoming(method: Method, uri: &str, media: &str, body: impl Into<Body>) -> Request {
    Request::from(
        ::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", media)
            .body(body.into())
            .unwrap(),
    )
}

#[tokio::test]
async fn direct_keys_preserve_source_precedence_and_all_input_access() {
    let mut request = incoming(
        Method::POST,
        "/?name=query&extra=url",
        "application/json",
        r#"{"name":"body","user":{"name":"Ada"},"items":[{"id":1},{"id":2}]}"#,
    );
    assert_eq!(request.input("name").await.unwrap(), Some(json!("body")));
    let dynamic_key = "name".to_owned();
    assert_eq!(
        request.input(dynamic_key.as_str()).await.unwrap(),
        Some(json!("body"))
    );
    assert_eq!(request.input("extra").await.unwrap(), Some(json!("url")));
    assert_eq!(
        request.input("user.name").await.unwrap(),
        Some(json!("Ada"))
    );
    assert_eq!(
        request.input("items.*.id").await.unwrap(),
        Some(json!([1, 2]))
    );
    assert_eq!(request.query("name").unwrap(), Some(json!("query")));
    assert_eq!(request.post("name").await.unwrap(), Some(json!("body")));
    assert_eq!(request.post("extra").await.unwrap(), None);
    assert_eq!(request.post("user.name").await.unwrap(), None);
    assert_eq!(
        request.input("name").await.unwrap(),
        request.input(Some("name")).await.unwrap()
    );
    let all = request.input(None).await.unwrap().unwrap();
    assert_eq!(all["name"], "body");
    assert_eq!(all["extra"], "url");
    assert_eq!(request.post(None).await.unwrap().unwrap()["name"], "body");
    assert_eq!(request.query(None).unwrap().unwrap()["name"], "query");
}

#[tokio::test]
async fn defaults_distinguish_absence_null_and_falsy_values() {
    let mut request = incoming(
        Method::POST,
        "/?empty=&zero=0&nested[key]=query",
        "application/json",
        r#"{"null":null,"empty":"","zero":0,"flag":false,"nested":{"key":null}}"#,
    );
    for (key, value) in [
        ("null", Value::Null),
        ("empty", json!("")),
        ("zero", json!(0)),
        ("flag", json!(false)),
        ("nested.key", Value::Null),
    ] {
        assert_eq!(request.input_or(key, "fallback").await.unwrap(), value);
    }
    assert_eq!(
        request.input_or("missing", "fallback").await.unwrap(),
        json!("fallback")
    );
    assert_eq!(
        request
            .input_or("missing", json!({"name":"default"}))
            .await
            .unwrap(),
        json!({"name":"default"})
    );
    assert_eq!(
        request.post_or("null", "fallback").await.unwrap(),
        json!("fallback")
    );
    assert_eq!(
        request.post_or("empty", "fallback").await.unwrap(),
        json!("")
    );
    assert_eq!(request.query_or("missing", 7).unwrap(), json!(7));
    assert_eq!(request.query_or("empty", "fallback").unwrap(), json!(""));
    assert_eq!(
        request.query_or("nested.key", "fallback").unwrap(),
        json!("fallback")
    );
    let mut query_request = incoming(Method::GET, "/", "application/x-www-form-urlencoded", "");
    query_request.merge(json!({"null":null})).await.unwrap();
    assert_eq!(query_request.query("null").unwrap(), Some(Value::Null));
    assert_eq!(
        query_request.query_or("null", "fallback").unwrap(),
        json!("fallback")
    );
}

#[tokio::test]
async fn typed_shortcuts_keep_defaults_and_conversion_rules() {
    let mut request = incoming(
        Method::POST,
        "/",
        "application/json",
        r#"{"n":"12x","fraction":"1.5x","flag":"yes","bad":"nonsense","label":" Ada ","null":null,"array":[1]}"#,
    );
    assert_eq!(request.integer("n").await.unwrap(), 12);
    assert_eq!(request.integer_strict("n", 42).await.unwrap(), 42);
    assert_eq!(request.integer_or("missing", 7).await.unwrap(), 7);
    assert_eq!(request.integer("missing").await.unwrap(), 0);
    assert_eq!(request.float("fraction").await.unwrap(), 1.5);
    assert_eq!(request.float_strict("fraction", 4.5).await.unwrap(), 4.5);
    assert_eq!(request.float_or("missing", 2.5).await.unwrap(), 2.5);
    assert_eq!(request.float("missing").await.unwrap(), 0.0);
    assert!(request.boolean("flag").await.unwrap());
    assert!(!request.boolean("bad").await.unwrap());
    assert!(request.boolean_or("missing", true).await.unwrap());
    assert!(request.boolean_strict("bad", true).await.unwrap());
    assert!(!request.boolean("missing").await.unwrap());
    assert_eq!(
        request.string("label").await.unwrap().trim().to_string(),
        "Ada"
    );
    assert_eq!(request.string("missing").await.unwrap().to_string(), "");
    assert_eq!(
        request
            .string_or("missing", "fallback")
            .await
            .unwrap()
            .to_string(),
        "fallback"
    );
    assert_eq!(request.string("array").await.unwrap().to_string(), "Array");
    assert_eq!(
        request
            .string_strict("array", "fallback")
            .await
            .unwrap()
            .to_string(),
        "fallback"
    );
    assert_eq!(
        request
            .string_or("null", "fallback")
            .await
            .unwrap()
            .to_string(),
        ""
    );
    assert_eq!(
        request
            .string_strict("null", "fallback")
            .await
            .unwrap()
            .to_string(),
        "fallback"
    );
    assert_eq!(request.str("label").await.unwrap().to_string(), " Ada ");
    assert_eq!(
        request
            .str_or("missing", "fallback")
            .await
            .unwrap()
            .to_string(),
        "fallback"
    );
    assert_eq!(
        request
            .str_strict("array", "fallback")
            .await
            .unwrap()
            .to_string(),
        "fallback"
    );
}

#[tokio::test]
async fn query_methods_and_form_body_keep_separate_sources() {
    for method in [Method::GET, Method::HEAD] {
        let mut request = incoming(
            method,
            "/?name=query",
            "application/x-www-form-urlencoded",
            "name=body",
        );
        assert_eq!(request.input("name").await.unwrap(), Some(json!("query")));
        assert_eq!(request.post("name").await.unwrap(), None);
    }
    let mut json_get = incoming(
        Method::GET,
        "/?name=query",
        "application/json",
        r#"{"name":"body"}"#,
    );
    assert_eq!(json_get.input("name").await.unwrap(), Some(json!("body")));
    assert_eq!(json_get.query("name").unwrap(), Some(json!("query")));
    let mut request = incoming(
        Method::POST,
        "/?name=query",
        "application/x-www-form-urlencoded",
        "name=body&profile[name]=Ada",
    );
    assert_eq!(request.input("name").await.unwrap(), Some(json!("body")));
    assert_eq!(
        request.input("profile.name").await.unwrap(),
        Some(json!("Ada"))
    );
    assert_eq!(request.query("name").unwrap(), Some(json!("query")));
}

#[tokio::test]
async fn default_helpers_do_not_hide_invalid_input_or_body_limits() {
    let mut request = incoming(Method::POST, "/", "application/json", "{broken");
    assert!(matches!(
        request.input_or("missing", "fallback").await,
        Err(HttpError::InvalidRequest)
    ));
    assert!(matches!(
        request.post_or("missing", "fallback").await,
        Err(HttpError::InvalidRequest)
    ));
    assert!(matches!(
        request.string("missing").await,
        Err(HttpError::InvalidRequest)
    ));
    let request = incoming(Method::GET, "/?name=%FF", "application/json", "");
    assert!(matches!(
        request.query_or("name", "fallback"),
        Err(HttpError::InvalidRequest)
    ));
    let mut request = incoming(
        Method::POST,
        "/",
        "application/json",
        " ".repeat(8 * 1024 * 1024 + 1),
    );
    assert!(matches!(
        request.input_or("missing", false).await,
        Err(HttpError::PayloadTooLarge)
    ));
    assert!(matches!(
        request.post_or("missing", false).await,
        Err(HttpError::PayloadTooLarge)
    ));
}

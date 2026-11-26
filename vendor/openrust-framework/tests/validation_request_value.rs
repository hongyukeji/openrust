use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    http::{Body, Request},
    validation::{Data, Factory, RuleSet},
};
use serde_json::json;

#[tokio::test]
async fn application_action_input_retains_nested_uploads_and_scalar_validation() {
    let body = concat!(
        "--input\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nAlice\r\n",
        "--input\r\nContent-Disposition: form-data; name=\"files[2]\"; filename=\"fixture.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n",
        "--input--\r\n"
    );
    let raw = axum::http::Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", "multipart/form-data; boundary=input")
        .body(Body::from(body))
        .unwrap();
    let (parts, body) = raw.into_parts();
    let mut request = Request::from_parts(parts, body);
    let native = Data::from_request(&mut request).await.unwrap();
    let values = request.all().await.unwrap();
    let actions = Data::from_request_value(&values).unwrap();
    assert_eq!(actions.as_value(), native.as_value());
    assert!(actions.file("files.2").is_some());
    for data in [native, actions] {
        let rules = RuleSet::parse([
            ("name", "required|string"),
            ("files.2", "required|file|max:1"),
        ])
        .unwrap();
        Factory::default()
            .make(data.clone(), rules, Default::default(), Default::default())
            .unwrap()
            .validate()
            .await
            .unwrap();
        let rules = RuleSet::parse([("files.2", "required|string")]).unwrap();
        let mut validator = Factory::default()
            .make(data, rules, Default::default(), Default::default())
            .unwrap();
        assert!(validator.validate().await.is_err());
    }
}

#[tokio::test]
async fn size_comparisons_use_uploaded_file_kilobytes_and_reject_scalar_peers() {
    let mut body = Vec::new();
    for (name, length) in [("large", 2048), ("small", 1024)] {
        body.extend_from_slice(format!("--sizes\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{name}.txt\"\r\nContent-Type: text/plain\r\n\r\n").as_bytes());
        body.extend(std::iter::repeat_n(b'x', length));
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(
        b"--sizes\r\nContent-Disposition: form-data; name=\"text\"\r\n\r\nxx\r\n--sizes--\r\n",
    );
    let raw = axum::http::Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", "multipart/form-data; boundary=sizes")
        .body(Body::from(body))
        .unwrap();
    let (parts, body) = raw.into_parts();
    let mut request = Request::from_parts(parts, body);
    let data = Data::from_request(&mut request).await.unwrap();
    for (field, rule, passes) in [
        ("large", "gt:small", true),
        ("large", "gte:large", true),
        ("small", "lt:large", true),
        ("small", "lte:small", true),
        ("large", "lt:small", false),
        ("large", "gt:text", false),
        ("large", "gte:missing", false),
        ("large", "gt:1", false),
    ] {
        let mut validator = Factory::default()
            .make(
                data.clone(),
                RuleSet::parse([(field, rule)]).unwrap(),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(validator.validate().await.is_ok(), passes, "{field} {rule}");
        if !passes {
            let message = validator.errors().first(Some(field), None);
            assert!(message.contains("kilobytes"), "{message}");
            if rule == "lt:small" {
                assert!(message.contains("than 1 kilobytes"), "{message}");
            }
        }
    }
}

#[tokio::test]
async fn setting_a_scalar_over_an_upload_removes_the_old_file_from_validated_input() {
    let body = concat!(
        "--input\r\nContent-Disposition: form-data; name=\"file\"; filename=\"old.txt\"\r\nContent-Type: text/plain\r\n\r\nold\r\n",
        "--input--\r\n"
    );
    let raw = axum::http::Request::builder()
        .method("POST")
        .uri("/")
        .header("content-type", "multipart/form-data; boundary=input")
        .body(Body::from(body))
        .unwrap();
    let (parts, body) = raw.into_parts();
    let mut request = Request::from_parts(parts, body);
    let data = Data::from_request(&mut request).await.unwrap();
    let mut validator = Factory::default()
        .make(
            data,
            RuleSet::parse([("file", "required|file")]).unwrap(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(validator.validate().await.unwrap().file("file").is_some());
    validator.set_value("file", json!("new")).unwrap();
    assert!(validator.safe().is_err());
    assert!(validator.get_data().file("file").is_none());
    validator.set_rules(RuleSet::parse([("file", "required|string")]).unwrap());
    let result = validator.validate().await.unwrap();
    assert_eq!(result.get("file"), Some(&json!("new")));
    assert!(result.file("file").is_none());
}

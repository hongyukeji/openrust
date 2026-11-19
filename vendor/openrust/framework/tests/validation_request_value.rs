use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    http::{Body, Request},
    validation::{Data, Factory, RuleSet},
};

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
    let native = Data::fromRequest(&mut request).await.unwrap();
    let values = request.all().await.unwrap();
    let actions = Data::fromRequestValue(&values).unwrap();
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

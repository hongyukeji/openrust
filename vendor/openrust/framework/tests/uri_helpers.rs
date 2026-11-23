use openrust::{http::Response, routing::RedirectOptions, support::Uri};
use serde_json::json;

#[test]
fn uri_components_form_validated_immutable_references() {
    let original = Uri::of("https://example.test/original#old").unwrap();
    let uri = original
        .with_scheme("http")
        .unwrap()
        .with_host("TEST.com")
        .unwrap()
        .with_port(Some(8000))
        .unwrap()
        .with_path("//users/你好")
        .unwrap()
        .with_fragment("mark 1")
        .unwrap();
    assert_eq!(
        uri.value(),
        "http://test.com:8000/users/%E4%BD%A0%E5%A5%BD#mark%201"
    );
    assert_eq!(uri.path_segments(), ["users", "%E4%BD%A0%E5%A5%BD"]);
    assert_eq!(original.value(), "https://example.test/original#old");
    assert!(uri.with_host("host@invalid").is_err());
    assert!(uri.with_port(Some(-1)).is_err());
    assert!(Uri::of("/relative").unwrap().with_scheme("https").is_err());
    assert_eq!(uri.with_port(Some(80)).unwrap().port(), None);
    let response: Response = uri.redirect(RedirectOptions::default()).unwrap().into();
    assert_eq!(response.headers()["location"], uri.value());
    assert!(
        Uri::of("javascript:alert(1)")
            .unwrap()
            .redirect(RedirectOptions::default())
            .is_err()
    );
}

#[test]
fn uri_queries_preserve_literals_and_apply_ordered_edits() {
    let uri =
        Uri::of("/items?label=a+b&exact.key=yes&filter%5B0%5D=active&filter%5B1%5D=pending#part")
            .unwrap();
    assert_eq!(uri.query().all().unwrap()["exact.key"], "yes");
    assert!(uri.query().has("exact.key").unwrap());
    assert_eq!(uri.query().get("label").unwrap(), Some(json!("a+b")));
    assert!(uri.query().decode().unwrap().contains("label=a+b"));
    let next = uri
        .with_query_if_missing(&json!({"label":"ignored","page":2}))
        .unwrap()
        .push_onto_query("filter", json!(["pending", "open"]))
        .unwrap()
        .without_query(&["exact.key"])
        .unwrap();
    assert_eq!(
        next.query().get("filter").unwrap(),
        Some(json!(["active", "pending", "open"]))
    );
    assert_eq!(next.query().get("page").unwrap(), Some(json!("2")));
    assert!(next.query().missing("exact.key").unwrap());
    assert!(next.query().all().unwrap().get("exact.key").is_none());
    assert!(next.value().contains("label=a%2Bb"));
    assert_eq!(
        next.replace_query(&json!({"a":[true,false],"s":"空"}))
            .unwrap()
            .value(),
        "/items?a%5B0%5D=1&a%5B1%5D=0&s=%E7%A9%BA#part"
    );
    assert_eq!(
        next.replace_query(&json!({})).unwrap().value(),
        "/items#part"
    );
    assert!(Uri::of("/?x=%FF").unwrap().query().all().is_err());
}

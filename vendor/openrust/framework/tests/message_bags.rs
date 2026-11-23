use openrust::support::{MessageBag, MessageLookup, ViewErrorBag};
use serde_json::json;

#[test]
fn construction_add_and_merge_have_distinct_duplicate_semantics() {
    let mut messages = MessageBag::new([("z", vec!["first", "first"]), ("a", vec![])]);
    assert_eq!(messages.keys(), ["z", "a"]);
    assert_eq!(messages.count(), 1);
    messages
        .add("z", "first")
        .addIf(false, "a", "ignored")
        .addIf(true, "a", "second");
    assert_eq!(messages.all(None), ["first", "second"]);
    messages.merge(&MessageBag::new([("z", ["first", "third"])]));
    assert_eq!(messages.count(), 4);
    assert_eq!(messages.all(None), ["first", "first", "third", "second"]);
    assert_eq!(messages.unique(None), ["first", "third", "second"]);
    messages.forget("z");
    assert_eq!(messages.keys(), ["a"]);
}

#[test]
fn wildcard_get_retains_keys_and_literal_wildcard_takes_precedence() {
    let mut messages = MessageBag::new([
        ("users.1.email", ["invalid"]),
        ("users.0.email", ["missing"]),
    ]);
    let selected = messages.get("users.*.email", Some(":key => :message"));
    assert!(matches!(selected, MessageLookup::Wildcard(_)));
    assert_eq!(
        serde_json::to_value(&selected).unwrap(),
        json!({"users.1.email":["users.1.email => invalid"],"users.0.email":["users.0.email => missing"]})
    );
    assert_eq!(messages.first(Some("users.*.email"), None), "invalid");
    messages.add("users.*.email", "literal");
    assert_eq!(
        messages.get("users.*.email", None),
        MessageLookup::Exact(vec!["literal".into()])
    );
    assert_eq!(messages.first(Some("absent"), None), "");
}

#[test]
fn predicates_empty_fields_and_format_follow_message_content() {
    let mut messages = MessageBag::new([("empty", Vec::<&str>::new())]);
    assert!(messages.is_empty());
    assert!(!messages.has(["empty"]));
    messages.add("blank", "").add("name", "invalid");
    assert!(messages.any());
    assert!(!messages.has("blank"));
    assert!(messages.has(None));
    assert!(messages.has([]));
    assert!(!messages.has_any([]));
    assert!(messages.has_any(["absent", "name"]));
    assert!(messages.missing(["empty", "absent"]));
    messages.setFormat("<:key>:message</:key>");
    assert!(messages.has("blank"));
    assert_eq!(messages.getFormat(), "<:key>:message</:key>");
    assert_eq!(
        messages.first(Some("name"), Some("")),
        "<name>invalid</name>"
    );
    assert_eq!(
        messages.first(Some("name"), Some("0")),
        "<name>invalid</name>"
    );
    assert_eq!(messages.first(Some("name"), Some(":message")), "invalid");
}

#[test]
fn raw_json_preserves_message_order_and_merged_duplicates() {
    let mut messages = MessageBag::new([("z", ["one"]), ("a", ["two"])]);
    messages.merge(&MessageBag::new([("z", ["one"])]));
    messages.setFormat("<li>:message</li>");
    assert_eq!(
        messages.to_json().unwrap(),
        r#"{"z":["one","one"],"a":["two"]}"#
    );
    let restored: MessageBag = serde_json::from_str(&messages.to_json().unwrap()).unwrap();
    assert_eq!(restored.all(None), ["one", "one", "two"]);
    assert_eq!(restored.getFormat(), ":message");
    assert_eq!(messages.to_array(), messages.jsonSerialize());
    assert_eq!(messages.getMessages(), messages.getMessageBag().messages());
    assert!(messages.to_pretty_json().unwrap().contains('\n'));
    assert!(serde_json::from_value::<MessageBag>(json!({"name":42})).is_err());
}

#[test]
fn named_bags_count_and_forward_only_the_default_bag() {
    let mut errors = ViewErrorBag::new();
    assert!(!errors.hasBag("default"));
    assert!(errors.getBag("missing").is_empty());
    assert!(errors.getBags().is_empty());
    errors.put("profile", MessageBag::new([("email", ["invalid"])]));
    assert_eq!(errors.count(), 0);
    assert!(!errors.any());
    assert_eq!(errors.first(None, None), "");
    errors.put(
        "default",
        MessageBag::new([("name", ["required", "short"])]),
    );
    assert_eq!(errors.count(), 2);
    assert!(errors.has("name"));
    assert_eq!(errors.first(None, None), "required");
    errors.put("profile", MessageBag::new([("email", ["changed"])]));
    errors.getBagMut("profile").unwrap().add("email", "second");
    assert!(errors.getBagMut("absent").is_none());
    assert_eq!(errors.getBag("profile").count(), 2);
    assert_eq!(errors.getBags().len(), 2);
    assert_eq!(errors.to_string(), r#"{"name":["required","short"]}"#);
    let restored: ViewErrorBag = serde_json::from_value(json!(errors)).unwrap();
    assert_eq!(restored, errors);
    let view: openrust::view::ViewValue = restored.into();
    assert_eq!(
        view["profile"]["email"][0],
        openrust::view::ViewValue::from("changed")
    );
}

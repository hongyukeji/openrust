use crate::Result;
use crate::support::{
    ArrayKey, BagRead, Collection, MessageBag, MessageBagContract, MessageJsonOptions, MessageKeys,
    MessageLookup, MessageValue, ViewErrorBag,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn bag(messages: &[(&str, &str)]) -> MessageBag {
    let mut bag = MessageBag::new();
    for (key, value) in messages {
        bag.add(*key, *value).expect("fixture");
    }
    bag
}

struct CustomBag(MessageBag, Option<usize>, Arc<AtomicUsize>);
impl Clone for CustomBag {
    fn clone(&self) -> Self {
        self.2.fetch_add(1, Ordering::SeqCst);
        Self(self.0.clone(), self.1, Arc::clone(&self.2))
    }
}
impl MessageBagContract for CustomBag {
    fn keys(&self) -> Vec<ArrayKey> {
        self.0.keys()
    }
    fn messages(&self) -> &Collection<Collection<MessageValue>> {
        self.0.messages()
    }
    fn to_array(&self) -> &Collection<Collection<MessageValue>> {
        if self.1 == Some(77) {
            static OVERRIDE: std::sync::OnceLock<Collection<Collection<MessageValue>>> =
                std::sync::OnceLock::new();
            OVERRIDE.get_or_init(|| {
                Collection::try_from_entries([(
                    "override".into(),
                    Collection::try_from_entries([(
                        0.into(),
                        MessageValue::from("array override"),
                    )])
                    .expect("inner fixture"),
                )])
                .expect("array fixture")
            })
        } else {
            self.0.to_array()
        }
    }
    fn to_json(&self, options: MessageJsonOptions) -> Result<String> {
        if self.1 == Some(77) {
            Ok("custom-json".into())
        } else {
            self.0.to_json(options)
        }
    }
    fn format(&self) -> &str {
        self.0.format()
    }
    fn set_format(&mut self, format: String) {
        self.0.set_format(format);
    }
    fn add(&mut self, key: ArrayKey, message: String) -> Result<()> {
        self.0.add(key, message)
    }
    fn merge(&mut self, messages: &Collection<Collection<MessageValue>>) -> Result<()> {
        self.0.merge(messages)
    }
    fn has(&self, keys: MessageKeys) -> Result<bool> {
        self.0.has(keys)
    }
    fn first(&self, key: Option<&str>, format: Option<&str>) -> Result<Option<String>> {
        self.0.first(key, format)
    }
    fn get(&self, key: &str, format: Option<&str>) -> Result<MessageLookup> {
        self.0.get(key, format)
    }
    fn all(&self, format: Option<&str>) -> Result<Collection<MessageValue>> {
        self.0.all(format)
    }
    fn forget(&mut self, key: &ArrayKey) {
        self.0.forget(key.clone());
    }
    fn count(&self) -> usize {
        self.1.unwrap_or_else(|| self.0.count())
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn is_not_empty(&self) -> bool {
        self.0.is_not_empty()
    }
}

#[test]
fn missing_bag_is_independent_and_not_inserted() {
    let mut view = ViewErrorBag::new();
    assert!(!view.has_bag("default"));
    assert_eq!(view.get_bag("missing").as_bag().count(), 0);
    assert!(matches!(view.get_bag("missing"), BagRead::Missing(_)));
    view.get_bag_mut("missing")
        .as_bag_mut()
        .add("a".into(), "x".into())
        .expect("temporary add");
    assert!(!view.has_bag("missing"));
    assert_eq!(view.get_bag("missing").as_bag().count(), 0);
    assert_eq!(view.to_string(), "[]");
    assert!(!view.default_has_any(MessageKeys::Any).expect("missing any"));
    assert!(
        view.default_missing(MessageKeys::Keys(vec!["a".into()]))
            .expect("missing")
    );
    assert!(!view.default_is_not_empty());
    view.put("default", Box::new(MessageBag::new()))
        .expect("empty default");
    assert!(!view.any());
    assert_eq!(view.count(), 0);
}
#[test]
fn put_get_replace_and_custom_identity_mutation() {
    let mut view = ViewErrorBag::new();
    view.put(
        "named",
        Box::new(CustomBag(
            bag(&[("a", "x")]),
            None,
            Arc::new(AtomicUsize::new(0)),
        )),
    )
    .expect("put");
    assert!(view.has_bag("named"));
    assert_eq!(view.bags().all().len(), 1);
    view.get_bag_mut("named")
        .as_bag_mut()
        .add("a".into(), "y".into())
        .expect("mutate stored");
    assert_eq!(view.get_bag("named").as_bag().count(), 2);
    view.put("named", Box::new(bag(&[("b", "z")])))
        .expect("replace");
    assert_eq!(view.bags().all().len(), 1);
    assert_eq!(
        view.get_bag("named")
            .as_bag()
            .first(Some("b"), None)
            .expect("first"),
        Some("z".into())
    );
}
#[test]
fn only_default_controls_count_any_and_display() {
    let mut view = ViewErrorBag::new();
    view.put("named", Box::new(bag(&[("a", "x")])))
        .expect("named");
    assert_eq!(view.count(), 0);
    assert!(!view.any());
    assert_eq!(view.to_string(), "[]");
    view.put("default", Box::new(bag(&[("a", "x"), ("a", "y")])))
        .expect("default");
    assert_eq!(view.count(), 2);
    assert!(view.any());
    assert_eq!(view.to_string(), r#"{"a":["x","y"]}"#);
    assert!(view.default_is_not_empty());
}
#[test]
fn default_proxies_use_stored_or_temporary() {
    let mut view = ViewErrorBag::new();
    view.default_add("a", "x").expect("temporary");
    assert!(!view.has_bag("default"));
    assert_eq!(
        view.default_first(None, None).expect("first"),
        Some(String::new())
    );
    view.put("default", Box::new(MessageBag::new()))
        .expect("put");
    view.default_add("a", "x").expect("stored add");
    assert!(view.default_is_not_empty());
    assert!(
        view.default_has_any(MessageKeys::Keys(vec!["a".into()]))
            .expect("has any")
    );
    assert!(
        !view
            .default_missing(MessageKeys::Keys(vec!["a".into()]))
            .expect("not missing")
    );
    assert_eq!(
        view.default_all(None)
            .expect("all")
            .to_value()
            .expect("value"),
        serde_json::json!(["x"])
    );
    assert_eq!(
        view.default_first(Some("a"), None).expect("first"),
        Some("x".into())
    );
    assert!(
        view.default_has(MessageKeys::Keys(vec!["a".into()]))
            .expect("has")
    );
    assert_eq!(
        view.default_get("a", None).expect("get"),
        MessageLookup::Exact(
            Collection::try_from_entries([(0.into(), "x".into())]).expect("expected")
        )
    );
    view.default_set_format(":key");
    assert_eq!(view.default_format(), ":key");
    view.default_forget(&"a".into());
    assert!(view.default_is_empty());
    let extra = Collection::try_from_entries([(
        "a".into(),
        Collection::try_from_entries([
            (0.into(), MessageValue::from("x")),
            (1.into(), MessageValue::from("x")),
        ])
        .expect("messages"),
    )])
    .expect("extra");
    view.default_merge(&extra).expect("merge default");
    assert_eq!(
        view.default_all(None)
            .expect("all")
            .to_value()
            .expect("value"),
        serde_json::json!(["a", "a"])
    );
    assert_eq!(
        view.default_unique(None)
            .expect("unique")
            .to_value()
            .expect("value"),
        serde_json::json!(["a"])
    );
    assert!(
        view.default_to_pretty_json(MessageJsonOptions::NONE)
            .expect("pretty")
            .contains('\n')
    );
}
#[test]
fn literal_dot_bag_precedes_path_and_has_bag_is_direct() {
    let mut view = ViewErrorBag::new();
    view.put("a.b", Box::new(bag(&[("x", "literal")])))
        .expect("literal");
    assert!(view.has_bag("a.b"));
    assert!(!view.has_bag("a"));
    assert_eq!(
        view.get_bag("a.b")
            .as_bag()
            .first(Some("x"), None)
            .expect("first"),
        Some("literal".into())
    );
}

#[test]
fn fixed_php_view_reference_cases_are_consumed() {
    let raw = include_str!("fixtures/laravel-message-bags/output.jsonl");
    let rows: Vec<serde_json::Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).expect("raw PHP JSONL"))
        .collect();
    assert_eq!(rows.len(), 10);
    assert_eq!(rows[8]["case"], "view_default_missing_literal");
    assert_eq!(rows[9]["case"], "view_default_only_count");
    let mut view = ViewErrorBag::new();
    view.put("named", Box::new(bag(&[("a", "x")])))
        .expect("named");
    assert_eq!(
        view.count(),
        usize::try_from(
            rows[8]["value"]["count_without_default"]
                .as_u64()
                .expect("count")
        )
        .expect("count fits usize")
    );
    assert_eq!(
        view.any(),
        rows[8]["value"]["any_without_default"]
            .as_bool()
            .expect("bool")
    );
    assert_eq!(
        view.has_bag("named"),
        rows[8]["value"]["named_exists"].as_bool().expect("bool")
    );
    view.get_bag_mut("missing")
        .as_bag_mut()
        .add("a".into(), "x".into())
        .expect("temporary");
    assert_eq!(
        view.has_bag("missing"),
        rows[8]["value"]["missing_exists_after_mutation"]
            .as_bool()
            .expect("bool")
    );
    assert_eq!(
        view.get_bag("missing")
            .as_bag()
            .to_array()
            .to_value()
            .expect("missing second get"),
        rows[8]["value"]["missing_second_get"]
    );
    view.put("foo.bar", Box::new(bag(&[("a", "literal")])))
        .expect("literal");
    assert_eq!(
        view.get_bag("foo.bar")
            .as_bag()
            .to_array()
            .to_value()
            .expect("literal bag"),
        rows[8]["value"]["literal_before_dot"]
    );
    view.put("foo", Box::new(MessageBag::new())).expect("foo");
    view.put("default", Box::new(bag(&[("a", "x"), ("a", "y")])))
        .expect("default");
    assert_eq!(
        view.count(),
        usize::try_from(rows[9]["value"]["count"].as_u64().expect("count"))
            .expect("count fits usize")
    );
    assert_eq!(view.any(), rows[9]["value"]["any"].as_bool().expect("bool"));
    let keys: Vec<_> = view.bags().iter().map(|(key, _)| key.to_string()).collect();
    assert_eq!(
        keys,
        rows[9]["value"]["all_bags"]
            .as_array()
            .expect("keys")
            .iter()
            .map(|value| value.as_str().expect("key"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn unrelated_put_preserves_custom_identity_and_dispatches_count() {
    let clone_count = Arc::new(AtomicUsize::new(0));
    let mut view = ViewErrorBag::new();
    view.put(
        "default",
        Box::new(CustomBag(
            bag(&[("a", "x")]),
            Some(77),
            Arc::clone(&clone_count),
        )),
    )
    .expect("custom");
    let before = {
        let read = view.get_bag("default");
        std::ptr::from_ref(read.as_bag()).cast::<()>()
    };
    view.put("other", Box::new(bag(&[("b", "y")])))
        .expect("unrelated put");
    let after = {
        let read = view.get_bag("default");
        std::ptr::from_ref(read.as_bag()).cast::<()>()
    };
    assert!(std::ptr::addr_eq(before, after));
    assert_eq!(clone_count.load(Ordering::SeqCst), 0);
    assert_eq!(view.count(), 77);
    assert!(view.any());
    assert_eq!(
        view.default_to_array().to_value().expect("override array"),
        serde_json::json!({"override":["array override"]})
    );
    assert_eq!(
        view.default_to_json(MessageJsonOptions::NONE)
            .expect("override json"),
        "custom-json"
    );
    assert_eq!(view.to_string(), "custom-json");
    assert_eq!(
        view.bags().to_value().expect("custom bag serialization")["default"],
        serde_json::json!({"override":["array override"]})
    );
    view.default_add("a", "z").expect("mutate original");
    assert_eq!(
        view.get_bag("default")
            .as_bag()
            .messages()
            .get("a")
            .expect("a")
            .all()
            .len(),
        2
    );
}

#[test]
fn put_budget_failure_keeps_existing_custom_identity() {
    let clone_count = Arc::new(AtomicUsize::new(0));
    let mut view = ViewErrorBag::new();
    view.put(
        "default",
        Box::new(CustomBag(
            bag(&[("a", "x")]),
            None,
            Arc::clone(&clone_count),
        )),
    )
    .expect("default");
    for index in 1..=1021 {
        view.put(format!("bag{index}"), Box::new(MessageBag::new()))
            .expect("within limit");
    }
    let before = {
        let read = view.get_bag("default");
        std::ptr::from_ref(read.as_bag()).cast::<()>()
    };
    assert!(view.put("overflow", Box::new(MessageBag::new())).is_err());
    let after = {
        let read = view.get_bag("default");
        std::ptr::from_ref(read.as_bag()).cast::<()>()
    };
    assert!(std::ptr::addr_eq(before, after));
    assert_eq!(clone_count.load(Ordering::SeqCst), 0);
    assert!(!view.has_bag("overflow"));
    assert_eq!(view.bags().all().len(), 1022);
}

#[test]
fn named_bag_order_and_default_aliases_match_upstream() {
    let mut view = ViewErrorBag::new();
    view.put("default", Box::new(MessageBag::new()))
        .expect("default");
    view.put("default2", Box::new(MessageBag::new()))
        .expect("default2");
    assert_eq!(
        view.bags()
            .all()
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>(),
        vec![ArrayKey::from("default"), ArrayKey::from("default2")]
    );
    assert_eq!(view.default_keys(), Vec::<ArrayKey>::new());
    assert!(view.default_is_empty());
    assert_eq!(
        view.default_to_json(crate::support::MessageJsonOptions::NONE)
            .expect("json"),
        "[]"
    );
    view.default_add_if(true, "field", "content")
        .expect("add if");
    assert_eq!(
        view.default_to_array().to_value().expect("value"),
        serde_json::json!({"field":["content"]})
    );
    assert_eq!(
        view.default_get_messages().to_value().expect("value"),
        view.default_messages().to_value().expect("same")
    );
}

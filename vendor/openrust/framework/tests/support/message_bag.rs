use crate::support::{
    ArrayKey, Collection, MessageBag, MessageInput, MessageJsonOptions, MessageKeys, MessageLookup,
    MessageValue,
};

fn key(value: &str) -> ArrayKey {
    ArrayKey::from(value)
}
fn many(values: impl IntoIterator<Item = (i64, &'static str)>) -> Collection<String> {
    Collection::try_from_entries(
        values
            .into_iter()
            .map(|(index, value)| (index.into(), value.to_owned())),
    )
    .expect("fixture")
}
fn raw_many(values: impl IntoIterator<Item = (i64, &'static str)>) -> Collection<MessageValue> {
    Collection::try_from_entries(
        values
            .into_iter()
            .map(|(index, value)| (index.into(), MessageValue::from(value))),
    )
    .expect("raw fixture")
}
fn bag(values: impl IntoIterator<Item = (&'static str, &'static str)>) -> MessageBag {
    let mut bag = MessageBag::new();
    for (name, message) in values {
        bag.add(name, message).expect("fixture");
    }
    bag
}

#[test]
fn empty_json_and_default_format() {
    let bag = MessageBag::new();
    assert_eq!(bag.to_json(MessageJsonOptions::NONE).expect("json"), "[]");
    assert_eq!(bag.to_string(), "[]");
    assert_eq!(bag.format(), ":message");
    assert_eq!(bag.count(), 0);
    assert!(bag.is_empty());
    assert!(!bag.is_not_empty());
    assert_eq!(
        bag.first(Some("missing"), None).expect("first"),
        Some(String::new())
    );
}
#[test]
fn constructor_per_key_scalar_and_sparse_unique() {
    let bag = MessageBag::try_from_messages([
        (
            key("a"),
            MessageInput::Many(many([(0, "x"), (1, "x"), (2, "y")])),
        ),
        (key("b"), MessageInput::One("z".into())),
    ])
    .expect("constructor");
    assert_eq!(
        bag.to_json(MessageJsonOptions::NONE).expect("json"),
        r#"{"a":{"0":"x","2":"y"},"b":["z"]}"#
    );
    assert_eq!(bag.count(), 3);
    assert_eq!(bag.keys(), vec![key("a"), key("b")]);
}
#[test]
fn numeric_outer_keys_keep_json_object_and_order() {
    let bag = MessageBag::try_from_messages([
        (2.into(), MessageInput::One("two".into())),
        (5.into(), MessageInput::One("five".into())),
    ])
    .expect("constructor");
    assert_eq!(
        bag.to_json(MessageJsonOptions::NONE).expect("json"),
        r#"{"2":["two"],"5":["five"]}"#
    );
}
#[test]
fn add_unique_add_if_and_merge_duplicates() {
    let mut bag = bag([("a", "x")]);
    bag.add("a", "x").expect("duplicate");
    bag.add_if(false, "a", "ignored").expect("false");
    bag.add_if(true, "a", "y").expect("true");
    assert_eq!(
        bag.get("a", None).expect("get"),
        MessageLookup::Exact(raw_many([(0, "x"), (1, "y")]))
    );
    let incoming = Collection::try_from_entries([(key("a"), raw_many([(0, "x"), (1, "x")]))])
        .expect("incoming");
    bag.merge(&incoming).expect("merge");
    assert_eq!(bag.count(), 4);
    assert_eq!(
        bag.all(None).expect("all").to_value().expect("values"),
        serde_json::json!(["x", "y", "x", "x"])
    );
    assert!(!bag.is_empty());
    assert!(bag.is_not_empty());
    assert_eq!(bag.to_string(), r#"{"a":["x","y","x","x"]}"#);
    assert_eq!(
        bag.to_json(MessageJsonOptions::NONE).expect("json"),
        r#"{"a":["x","y","x","x"]}"#
    );
}
#[test]
fn exact_wildcard_first_null_and_literal_priority() {
    let bag = MessageBag::try_from_messages([
        (key("a.one"), MessageInput::Many(Collection::default())),
        (key("a.two"), MessageInput::One("x".into())),
        (key("a.*"), MessageInput::One("literal".into())),
    ])
    .expect("bag");
    assert_eq!(
        bag.first(Some("a.*"), None).expect("first"),
        Some("literal".into())
    );
    let bag = MessageBag::try_from_messages([
        (key("a.one"), MessageInput::Many(Collection::default())),
        (key("a.two"), MessageInput::One("x".into())),
    ])
    .expect("bag");
    assert_eq!(bag.first(Some("a.*"), None).expect("first"), None);
    assert!(bag.has(MessageKeys::Keys(vec![key("a.*")])).expect("has"));
    assert_eq!(
        bag.first(Some("missing"), None).expect("missing"),
        Some(String::new())
    );
}
#[test]
fn formatting_controls_first_has_and_zero_fallback() {
    let mut bag = bag([("a", "")]);
    assert!(!bag.has(MessageKeys::Keys(vec![key("a")])).expect("has"));
    bag.set_format(":key");
    assert_eq!(bag.format(), ":key");
    assert!(
        bag.has(MessageKeys::Keys(vec![key("a")]))
            .expect("has formatted")
    );
    assert_eq!(bag.first(Some("a"), None).expect("first"), Some("a".into()));
    bag.set_format("[:key] :message");
    assert_eq!(
        bag.all(None).expect("all").to_value().expect("value"),
        serde_json::json!(["[a] "])
    );
    assert_eq!(
        bag.first(Some("a"), Some(":message")).expect("explicit"),
        Some(String::new())
    );
    assert_eq!(
        bag.first(Some("a"), Some("")).expect("empty fallback"),
        Some("[a] ".into())
    );
    assert_eq!(
        bag.first(Some("a"), Some("0")).expect("zero fallback"),
        Some("[a] ".into())
    );
}
#[test]
fn all_unique_sparse_indices_forget_and_presence() {
    let mut bag = bag([("a", "x"), ("b", "y"), ("c", "x"), ("d", "z")]);
    assert_eq!(
        bag.unique(None).expect("unique").all(),
        &[
            (0.into(), "x".into()),
            (1.into(), "y".into()),
            (3.into(), "z".into())
        ]
    );
    assert!(bag.has(MessageKeys::Keys(vec![])).expect("empty all"));
    assert!(!bag.has_any(MessageKeys::Keys(vec![])).expect("empty any"));
    assert!(
        bag.missing(MessageKeys::Keys(vec![key("missing")]))
            .expect("missing")
    );
    bag.forget("b");
    bag.add("b", "new").expect("readd");
    assert_eq!(bag.keys(), vec![key("a"), key("c"), key("d"), key("b")]);
}
#[test]
fn json_flags_numeric_unicode_slash_hex_and_force_object() {
    let bag = bag([("a", "123"), ("b", "é/<&'\"")]);
    let numeric = bag
        .to_json(MessageJsonOptions::NUMERIC_CHECK)
        .expect("numeric");
    assert!(numeric.contains("[123]"));
    let pretty = bag
        .to_pretty_json(MessageJsonOptions::NONE)
        .expect("pretty");
    assert!(pretty.contains("\n    "));
    let unescaped = bag
        .to_json(MessageJsonOptions::UNESCAPED_UNICODE.union(MessageJsonOptions::UNESCAPED_SLASHES))
        .expect("unescaped");
    assert!(unescaped.contains("é/"));
    let hex = bag
        .to_json(
            MessageJsonOptions::HEX_TAG
                .union(MessageJsonOptions::HEX_AMP)
                .union(MessageJsonOptions::HEX_APOS)
                .union(MessageJsonOptions::HEX_QUOT),
        )
        .expect("hex");
    assert!(
        hex.contains("\\u003C")
            && hex.contains("\\u0026")
            && hex.contains("\\u0027")
            && hex.contains("\\u0022")
    );
    assert_eq!(
        MessageBag::new()
            .to_json(MessageJsonOptions::FORCE_OBJECT)
            .expect("force"),
        "{}"
    );
}

#[test]
#[allow(clippy::too_many_lines, reason = "逐字段对照十行固定 PHP 原始行为样本")]
fn fixed_php_reference_cases_are_named_and_used() {
    let raw = include_str!("fixtures/laravel-message-bags/output.jsonl");
    let rows: Vec<serde_json::Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).expect("raw PHP JSONL"))
        .collect();
    assert_eq!(rows.len(), 10);
    let names: Vec<&str> = rows
        .iter()
        .map(|row| row["case"].as_str().expect("case"))
        .collect();
    assert_eq!(
        names,
        [
            "empty_json",
            "numeric_outer_keys",
            "constructor_unique_sparse",
            "add_vs_merge",
            "first_wildcard_empty_first_group",
            "format_empty_zero_fallback",
            "has_formatted_result",
            "unique_sparse_indices",
            "view_default_missing_literal",
            "view_default_only_count"
        ]
    );
    assert_eq!(
        MessageBag::new()
            .to_json(MessageJsonOptions::NONE)
            .expect("empty"),
        rows[0]["value"]["json"].as_str().expect("expected")
    );
    assert_eq!(
        MessageBag::new().to_string(),
        rows[0]["value"]["serialized"].as_str().expect("serialized")
    );
    assert_eq!(
        MessageBag::new().count(),
        usize::try_from(rows[0]["value"]["count"].as_u64().expect("count"))
            .expect("count fits usize")
    );
    let numeric = MessageBag::try_from_messages([
        (2.into(), MessageInput::One("two".into())),
        (5.into(), MessageInput::One("five".into())),
    ])
    .expect("numeric");
    assert_eq!(
        numeric
            .to_json(MessageJsonOptions::NONE)
            .expect("numeric json"),
        rows[1]["value"]["json"].as_str().expect("expected")
    );
    assert_eq!(
        numeric.to_array().to_value().expect("raw"),
        rows[1]["value"]["raw"]
    );
    assert_eq!(
        numeric
            .keys()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        rows[1]["value"]["keys"]
            .as_array()
            .expect("keys")
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    let sparse = MessageBag::try_from_messages([(
        key("a"),
        MessageInput::Many(many([(0, "x"), (1, "x"), (2, "y")])),
    )])
    .expect("sparse");
    assert_eq!(
        sparse.count(),
        usize::try_from(rows[2]["value"]["count"].as_u64().expect("count"))
            .expect("count fits usize")
    );
    assert_eq!(
        sparse.to_array().to_value().expect("raw"),
        rows[2]["value"]["raw"]
    );
    assert_eq!(
        sparse.all(None).expect("all").to_value().expect("value"),
        rows[2]["value"]["all"]
    );
    let mut added = bag([("a", "x")]);
    added.add("a", "x").expect("duplicate");
    added.add("a", "y").expect("second");
    assert_eq!(
        added.to_array().to_value().expect("add"),
        rows[3]["value"]["add"]
    );
    let incoming =
        Collection::try_from_entries([(key("a"), raw_many([(0, "x"), (1, "y"), (2, "x")]))])
            .expect("incoming");
    let mut merged = bag([("a", "x")]);
    merged.merge(&incoming).expect("merge");
    assert_eq!(
        merged.to_array().to_value().expect("merge raw"),
        rows[3]["value"]["merge"]
    );
    assert_eq!(
        merged
            .all(None)
            .expect("merge all")
            .to_value()
            .expect("value"),
        rows[3]["value"]["merge_all"]
    );
    let wildcard = MessageBag::try_from_messages([
        (key("a.one"), MessageInput::Many(Collection::default())),
        (key("a.two"), MessageInput::One("x".into())),
    ])
    .expect("wildcard");
    assert!(rows[4]["value"]["first"].is_null());
    assert_eq!(wildcard.first(Some("a.*"), None).expect("first"), None);
    match wildcard.get("a.*", None).expect("get") {
        MessageLookup::Wildcard(groups) => {
            assert_eq!(groups.to_value().expect("groups"), rows[4]["value"]["get"]);
        }
        other => assert!(
            matches!(other, MessageLookup::Wildcard(_)),
            "expected wildcard groups, got {other:?}"
        ),
    }
    assert_eq!(
        wildcard
            .has(MessageKeys::Keys(vec![key("a.*")]))
            .expect("has"),
        rows[4]["value"]["has"].as_bool().expect("bool")
    );
    let mut formatted = bag([("a", "x")]);
    formatted.set_format("[:key] :message");
    assert_eq!(
        formatted.get("a", Some("")).expect("empty format"),
        MessageLookup::Exact(
            Collection::try_from_entries([(
                0.into(),
                rows[5]["value"]["empty"][0]
                    .as_str()
                    .expect("text")
                    .to_owned()
                    .into()
            )])
            .expect("expected")
        )
    );
    assert_eq!(
        formatted.get("a", Some("0")).expect("zero format"),
        MessageLookup::Exact(
            Collection::try_from_entries([(
                0.into(),
                rows[5]["value"]["zero"][0]
                    .as_str()
                    .expect("text")
                    .to_owned()
                    .into()
            )])
            .expect("expected")
        )
    );
    assert_eq!(
        formatted.format(),
        rows[5]["value"]["format"].as_str().expect("format")
    );
    match formatted
        .get("a", Some("<:message>"))
        .expect("explicit format")
    {
        MessageLookup::Exact(values) => assert_eq!(
            values.to_value().expect("value"),
            rows[5]["value"]["explicit"]
        ),
        other => assert!(
            matches!(other, MessageLookup::Exact(_)),
            "expected exact messages, got {other:?}"
        ),
    }
    let mut empty = bag([("a", "")]);
    assert_eq!(
        empty
            .has(MessageKeys::Keys(vec![key("a")]))
            .expect("default has"),
        rows[6]["value"]["default"].as_bool().expect("bool")
    );
    empty.set_format(":key");
    assert_eq!(
        empty.format(),
        rows[6]["value"]["format"].as_str().expect("format")
    );
    assert_eq!(
        empty.has(MessageKeys::Keys(vec![key("a")])).expect("has"),
        rows[6]["value"]["after"].as_bool().expect("bool")
    );
    assert_eq!(
        empty.first(Some("a"), None).expect("first"),
        rows[6]["value"]["first"].as_str().map(str::to_owned)
    );
    let unique = bag([("a", "x"), ("b", "y"), ("c", "x"), ("d", "z")]);
    assert_eq!(
        unique.all(None).expect("all").to_value().expect("value"),
        rows[7]["value"]["all"]
    );
    assert_eq!(
        unique
            .unique(None)
            .expect("unique")
            .to_value()
            .expect("value"),
        rows[7]["value"]["unique"]
    );
}

#[test]
fn fixed_php_json_flag_matrix_for_string_messages() {
    let raw = include_str!("fixtures/laravel-message-bags/json_flags.jsonl");
    let rows: Vec<serde_json::Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).expect("raw PHP JSONL"))
        .collect();
    assert_eq!(rows.len(), 6);
    let expected_names = [
        "default",
        "unicode_slashes",
        "hex",
        "numeric",
        "fraction",
        "all",
    ];
    let bag = bag([("field", "汉字/a<b>&\"'"), ("field", "12")]);
    for (row, name) in rows.iter().zip(expected_names) {
        assert_eq!(row["case"], name);
        let flags = u32::try_from(row["flags"].as_u64().expect("flags")).expect("flags fit u32");
        let options = MessageJsonOptions::from_bits(flags).expect("supported PHP flags");
        let raw_expected = row["json"]
            .as_str()
            .expect("original PHP JSON with mixed float");
        let (string_prefix, mixed_tail) = raw_expected.rsplit_once(',').expect("mixed third item");
        assert!(
            matches!(mixed_tail, "1]}" | "1.0]}"),
            "preserve mixed PHP fixture"
        );
        let expected = format!("{string_prefix}]}}");
        assert_eq!(
            bag.to_json(options).expect("encode"),
            expected,
            "flag case {name}"
        );
    }
}

#[test]
fn upstream_provider_arrayable_and_wildcard_group_matrix() {
    let arrayable = MessageBag::try_from_messages([
        (
            0.into(),
            MessageInput::Arrayable(many([(0, "foo"), (1, "bar")])),
        ),
        (
            1.into(),
            MessageInput::Arrayable(many([(0, "baz"), (1, "qux")])),
        ),
    ])
    .expect("arrayable per key");
    assert_eq!(
        arrayable.to_array().to_value().expect("value"),
        serde_json::json!([["foo", "bar"], ["baz", "qux"]])
    );
    let mut target = bag([("foo", "bar")]);
    let source = bag([("foo", "baz"), ("bar", "foo")]);
    target.merge_provider(&source).expect("provider merge");
    assert_eq!(
        target.to_json(MessageJsonOptions::NONE).expect("json"),
        r#"{"foo":["bar","baz"],"bar":["foo"]}"#
    );
    let dotted = bag([("foo.1", "bar"), ("foo.2", "baz")]);
    match dotted.get("foo.*", None).expect("wildcard get") {
        MessageLookup::Wildcard(groups) => assert_eq!(
            groups.to_value().expect("groups"),
            serde_json::json!({"foo.1":["bar"],"foo.2":["baz"]})
        ),
        other => assert!(
            matches!(other, MessageLookup::Wildcard(_)),
            "must retain wildcard shape"
        ),
    }
    assert_eq!(
        dotted.first(Some("foo.*"), None).expect("first"),
        Some("bar".into())
    );
}

#[test]
fn upstream_has_has_any_missing_key_variants() {
    let mut bag = MessageBag::new();
    assert!(!bag.has(MessageKeys::Any).expect("empty any"));
    assert!(!bag.has_any(MessageKeys::Any).expect("empty any"));
    bag.add("foo", "bar").expect("add");
    bag.add("bar", "foo").expect("add");
    bag.add("boom", "baz").expect("add");
    assert!(bag.has(MessageKeys::Any).expect("null"));
    assert!(bag.has_any(MessageKeys::Any).expect("null"));
    assert!(
        bag.has(MessageKeys::Keys(vec![key("foo"), key("bar"), key("boom")]))
            .expect("all")
    );
    assert!(
        !bag.has(MessageKeys::Keys(vec![key("foo"), key("bad")]))
            .expect("all")
    );
    assert!(
        bag.has_any(MessageKeys::Keys(vec![key("bad"), key("foo")]))
            .expect("any")
    );
    assert!(
        !bag.has_any(MessageKeys::Keys(vec![key("bad"), key("biz")]))
            .expect("any")
    );
    assert!(
        !bag.missing(MessageKeys::Keys(vec![key("foo"), key("bad")]))
            .expect("missing")
    );
    assert!(
        bag.missing(MessageKeys::Keys(vec![key("bad"), key("biz")]))
            .expect("missing")
    );
}

#[test]
fn budget_errors_leave_messages_unchanged() {
    let many = Collection::try_from_entries(
        (0..1023_i64).map(|index| (index.into(), format!("m{index}"))),
    )
    .expect("limit fixture");
    let mut bag =
        MessageBag::try_from_messages([(key("a"), MessageInput::Many(many))]).expect("bag");
    let before = bag.to_array().to_value().expect("before");
    assert!(bag.add("a", "overflow").is_err());
    assert_eq!(bag.to_array().to_value().expect("after"), before);
}

#[test]
fn numeric_json_fraction_and_overflow_are_explicit() {
    let fractional = bag([("field", "1.0")]);
    assert_eq!(
        fractional
            .to_json(MessageJsonOptions::NUMERIC_CHECK)
            .expect("numeric"),
        r#"{"field":[1]}"#
    );
    assert_eq!(
        fractional
            .to_json(
                MessageJsonOptions::NUMERIC_CHECK.union(MessageJsonOptions::PRESERVE_ZERO_FRACTION)
            )
            .expect("fraction"),
        r#"{"field":[1.0]}"#
    );
    let overflow = bag([("field", "1e999")]);
    assert_eq!(
        overflow
            .to_json(MessageJsonOptions::NUMERIC_CHECK)
            .expect("overflow remains string"),
        r#"{"field":["1e999"]}"#
    );
}

#[test]
fn unicode_line_terminators_require_both_flags() {
    let bag = bag([("field", "a\u{2028}b")]);
    assert!(
        bag.to_json(MessageJsonOptions::UNESCAPED_UNICODE)
            .expect("unicode")
            .contains("\\u2028")
    );
    assert!(
        bag.to_json(
            MessageJsonOptions::UNESCAPED_UNICODE
                .union(MessageJsonOptions::UNESCAPED_LINE_TERMINATORS)
        )
        .expect("terminators")
        .contains("a\u{2028}b")
    );
}

#[test]
fn add_uses_php_loose_numeric_string_equality_but_constructor_and_unique_use_string_comparison() {
    let mut added = MessageBag::new();
    added.add("n", "0").expect("zero");
    added.add("n", "00").expect("loose duplicate");
    added.add("n", "1e2").expect("hundred");
    added.add("n", "100").expect("loose duplicate");
    assert_eq!(added.count(), 2);
    let constructed = MessageBag::try_from_messages([(
        key("n"),
        MessageInput::Many(many([(0, "0"), (1, "00"), (2, "1e2"), (3, "100")])),
    )])
    .expect("constructor");
    assert_eq!(constructed.count(), 4);
    assert_eq!(constructed.unique(None).expect("unique").all().len(), 4);
}

#[test]
fn merge_numeric_outer_keys_appends_instead_of_merging_by_same_key() {
    let mut target = MessageBag::try_from_messages([(0.into(), MessageInput::One("first".into()))])
        .expect("target");
    let incoming =
        Collection::try_from_entries([(0.into(), raw_many([(0, "second")]))]).expect("incoming");
    target.merge(&incoming).expect("merge");
    assert_eq!(
        target.to_json(MessageJsonOptions::NONE).expect("json"),
        r#"[["first"],["second"]]"#
    );
}

#[test]
fn merge_reindexes_existing_sparse_numeric_keys_even_when_incoming_is_empty() {
    let mut outer =
        MessageBag::try_from_messages([(2.into(), MessageInput::One("a".into()))]).expect("outer");
    outer.merge(&Collection::default()).expect("empty merge");
    assert_eq!(
        outer.to_array().to_value().expect("outer raw"),
        serde_json::json!([["a"]])
    );
    let incoming =
        Collection::try_from_entries([(5.into(), raw_many([(0, "b")]))]).expect("incoming");
    outer.merge(&incoming).expect("outer append");
    assert_eq!(
        outer.to_array().to_value().expect("outer raw"),
        serde_json::json!([["a"], ["b"]])
    );

    let mut inner =
        MessageBag::try_from_messages([(key("field"), MessageInput::Many(many([(2, "a")])))])
            .expect("inner");
    let empty_inner =
        Collection::try_from_entries([(key("field"), Collection::<MessageValue>::default())])
            .expect("empty inner");
    inner.merge(&empty_inner).expect("empty inner merge");
    assert_eq!(
        inner.to_array().to_value().expect("inner raw"),
        serde_json::json!({"field":{"2":"a"}})
    );
    let incoming_inner = Collection::try_from_entries([(
        key("field"),
        Collection::try_from_entries([(5.into(), MessageValue::from("b"))]).expect("inner values"),
    )])
    .expect("incoming inner");
    inner.merge(&incoming_inner).expect("inner append");
    assert_eq!(
        inner.to_array().to_value().expect("inner raw"),
        serde_json::json!({"field":{"2":"a", "3":"b"}})
    );
}

#[test]
fn merge_preserves_inner_string_keys_and_nested_conflict() {
    let mut target = MessageBag::try_from_messages([(
        key("field"),
        MessageInput::Many(
            Collection::try_from_entries([(key("left"), "x".into())]).expect("left"),
        ),
    )])
    .expect("target");
    let add = Collection::try_from_entries([(
        key("field"),
        Collection::try_from_entries([(key("right"), "y".into())]).expect("right"),
    )])
    .expect("add");
    target.merge(&add).expect("merge");
    assert_eq!(
        target.to_json(MessageJsonOptions::NONE).expect("json"),
        r#"{"field":{"left":"x","right":"y"}}"#
    );
    assert_eq!(target.count(), 2);
    assert!(target.all(None).is_err());
    let conflicting = Collection::try_from_entries([(
        key("field"),
        Collection::try_from_entries([(key("left"), "z".into())]).expect("collision"),
    )])
    .expect("incoming");
    target.merge(&conflicting).expect("nested merge");
    assert_eq!(target.count(), 4);
    assert_eq!(
        target
            .to_json(MessageJsonOptions::NONE)
            .expect("nested json"),
        r#"{"field":{"left":["x","z"],"right":"y"}}"#
    );
    assert_eq!(
        target.first(Some("field"), None).expect("nested first"),
        Some("x".into())
    );
}

#[test]
fn constructor_and_add_agree_for_same_string_duplicates() {
    let constructed = MessageBag::try_from_messages([(
        key("messages"),
        MessageInput::Many(many([
            (0, "first"),
            (1, "second"),
            (2, "third"),
            (3, "third"),
        ])),
    )])
    .expect("constructor");
    let mut added = MessageBag::new();
    for message in ["first", "second", "third", "third"] {
        added.add("messages", message).expect("add");
    }
    assert_eq!(
        constructed.to_array().to_value().expect("constructor raw"),
        added.to_array().to_value().expect("added raw")
    );
}

#[test]
#[allow(clippy::too_many_lines, reason = "逐字段对照三十一行固定 PHP 边界样本")]
fn fixed_php_edge_reference_is_consumed() {
    let raw = include_str!("fixtures/laravel-message-bags/edge_output.jsonl");
    let rows: Vec<serde_json::Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).expect("raw PHP edge JSONL"))
        .collect();
    assert_eq!(rows.len(), 31);
    for (index, (left, right)) in [("0", "00"), ("1", "01"), ("1e2", "100")]
        .into_iter()
        .enumerate()
    {
        let mut added = MessageBag::new();
        added.add("field", left).expect("left");
        added.add("field", right).expect("right");
        assert_eq!(
            added.to_array().to_value().expect("added"),
            rows[index]["value"]["add"]
        );
        let constructed = MessageBag::try_from_messages([(
            key("field"),
            MessageInput::Many(
                Collection::try_from_entries([(0.into(), left.into()), (1.into(), right.into())])
                    .expect("pair"),
            ),
        )])
        .expect("constructor");
        assert_eq!(
            constructed.to_array().to_value().expect("constructed"),
            rows[index]["value"]["constructor"]
        );
        assert_eq!(
            constructed
                .unique(None)
                .expect("unique")
                .to_value()
                .expect("value"),
            rows[index]["value"]["unique"]
        );
    }
    let mut numeric = MessageBag::try_from_messages([(0.into(), MessageInput::One("a".into()))])
        .expect("numeric");
    numeric
        .merge(
            &Collection::try_from_entries([(
                0.into(),
                Collection::try_from_entries([(0.into(), "b".into())]).expect("b"),
            )])
            .expect("incoming"),
        )
        .expect("merge");
    assert_eq!(
        numeric.to_array().to_value().expect("numeric raw"),
        rows[3]["value"]["raw"]
    );
    assert_eq!(
        numeric.to_json(MessageJsonOptions::NONE).expect("json"),
        rows[3]["value"]["json"].as_str().expect("json")
    );
    for (key, field) in [("0", "get0"), ("1", "get1")] {
        match numeric.get(key, None).expect("get") {
            MessageLookup::Exact(values) => {
                assert_eq!(values.to_value().expect("value"), rows[3]["value"][field]);
            }
            other => assert!(
                matches!(other, MessageLookup::Exact(_)),
                "expected exact messages, got {other:?}"
            ),
        }
    }
    assert_eq!(
        numeric.all(None).expect("all").to_value().expect("value"),
        rows[3]["value"]["all"]
    );
    assert_eq!(
        numeric.count(),
        usize::try_from(rows[3]["value"]["count"].as_u64().expect("count"))
            .expect("count fits usize")
    );
    for (index, old_key, new_key) in [(4, 0, 0), (5, 1, 2), (6, 1, 1)] {
        let old_inner = if old_key == 0 {
            ArrayKey::Integer(0)
        } else {
            key(if index == 6 { "label" } else { "old" })
        };
        let new_inner = if new_key == 0 {
            ArrayKey::Integer(0)
        } else {
            key(if index == 6 { "label" } else { "new" })
        };
        let mut merged = MessageBag::try_from_messages([(
            key("field"),
            MessageInput::Many(
                Collection::try_from_entries([(
                    old_inner,
                    if index == 4 { "a" } else { "x" }.to_owned(),
                )])
                .expect("old"),
            ),
        )])
        .expect("bag");
        let incoming = Collection::try_from_entries([(
            key("field"),
            Collection::try_from_entries([(
                new_inner,
                MessageValue::Text(if index == 4 { "b" } else { "y" }.into()),
            )])
            .expect("new"),
        )])
        .expect("incoming");
        merged.merge(&incoming).expect("merge");
        assert_eq!(
            merged.to_array().to_value().expect("raw"),
            rows[index]["value"]["raw"]["value"]
        );
        match merged.get("field", None).expect("get") {
            MessageLookup::Exact(values) => assert_eq!(
                values.to_value().expect("get value"),
                rows[index]["value"]["get"]["value"]
            ),
            other => assert!(
                matches!(other, MessageLookup::Exact(_)),
                "expected exact messages, got {other:?}"
            ),
        }
        assert_eq!(
            merged.to_json(MessageJsonOptions::NONE).expect("json"),
            rows[index]["value"]["json"]["value"]
                .as_str()
                .expect("json")
        );
        assert_eq!(
            merged.count(),
            usize::try_from(
                rows[index]["value"]["count"]["value"]
                    .as_u64()
                    .expect("count"),
            )
            .expect("count fits usize")
        );
        if index == 4 {
            assert_eq!(
                merged.all(None).expect("all").to_value().expect("value"),
                rows[index]["value"]["all"]["value"]
            );
        } else {
            let error = merged.all(None).expect_err("PHP named inner key error");
            assert_eq!(error.code(), "message_all_named_argument");
            assert_eq!(
                rows[index]["value"]["all"]["exception"],
                "ArgumentCountError"
            );
        }
    }
    for case in ["1e1000", "-1e1000", "1.0", "1e-1000"] {
        let candidate = bag([("field", case)]);
        for (suffix, flags) in [
            ("0", 0),
            ("numeric", 32),
            ("numeric_throw", 4_194_336),
            ("numeric_partial", 544),
            ("numeric_partial_throw", 4_194_848),
        ] {
            let row = rows
                .iter()
                .find(|row| row["case"] == format!("json_{case}_{suffix}"))
                .expect("numeric case");
            let encoded = candidate
                .to_json(MessageJsonOptions::from_bits(flags).expect("flags"))
                .expect("encode");
            assert_eq!(
                encoded,
                row["value"]["json"].as_str().expect("json"),
                "{case}/{suffix}"
            );
        }
    }
    let line = bag([("field", "before\u{2028}middle\u{2029}after")]);
    for (name, flags) in [
        ("unicode_line_0", 0),
        ("unicode_line_unicode", 256),
        ("unicode_line_line", 2048),
        ("unicode_line_unicode_line", 2304),
    ] {
        let row = rows
            .iter()
            .find(|row| row["case"] == name)
            .expect("line case");
        assert_eq!(
            line.to_json(MessageJsonOptions::from_bits(flags).expect("flags"))
                .expect("line json"),
            row["value"]["json"].as_str().expect("expected")
        );
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "逐字段对照两模式各四十五行固定 PHP 样本"
)]
fn fixed_php_sparse_merge_and_laravel_nested_format_reference() {
    use serde_json::{Value, json};
    let bare: Vec<Value> = include_str!("fixtures/laravel-message-bags/sparse_format_bare.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("bare PHP line"))
        .collect();
    let managed: Vec<Value> =
        include_str!("fixtures/laravel-message-bags/sparse_format_laravel_error.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).expect("Laravel error handler line"))
            .collect();
    assert_eq!(bare.len(), 45);
    assert_eq!(managed.len(), 45);
    for (plain, framework) in bare.iter().zip(&managed) {
        assert_eq!(plain["mode"], "bare");
        assert_eq!(framework["mode"], "laravel_handle_error_method");
        assert_eq!(plain["case"], framework["case"]);
    }
    let compare = |name: &str, actual: crate::Result<Value>| {
        let row = managed
            .iter()
            .find(|row| row["case"] == name)
            .expect("named PHP case");
        if let Some(expected) = row["result"].get("value") {
            assert_eq!(actual.expect(name), *expected, "{name}");
        } else {
            let error = actual.expect_err(name);
            let expected = row["result"]["exception"].as_str().expect("exception");
            assert_eq!(
                error.code(),
                match expected {
                    "ErrorException" => "message_format_nested_value",
                    "ArgumentCountError" => "message_all_named_argument",
                    other => {
                        assert!(
                            matches!(other, "ErrorException" | "ArgumentCountError"),
                            "unexpected PHP exception {other}"
                        );
                        "unknown_reference_exception"
                    }
                },
                "{name}"
            );
        }
    };
    let mut outer =
        MessageBag::try_from_messages([(2.into(), MessageInput::One("a".into()))]).expect("outer");
    outer
        .merge(&Collection::try_from_entries([(5.into(), raw_many([(0, "b")]))]).expect("incoming"))
        .expect("outer merge");
    let mut outer_empty =
        MessageBag::try_from_messages([(2.into(), MessageInput::One("a".into()))])
            .expect("outer empty");
    outer_empty
        .merge(&Collection::default())
        .expect("outer empty merge");
    let mut inner =
        MessageBag::try_from_messages([(key("field"), MessageInput::Many(many([(2, "a")])))])
            .expect("inner");
    inner
        .merge(
            &Collection::try_from_entries([(key("field"), raw_many([(5, "b")]))])
                .expect("incoming"),
        )
        .expect("inner merge");
    let mut inner_empty =
        MessageBag::try_from_messages([(key("field"), MessageInput::Many(many([(2, "a")])))])
            .expect("inner empty");
    inner_empty
        .merge(
            &Collection::try_from_entries([(key("field"), Collection::default())])
                .expect("incoming"),
        )
        .expect("inner empty merge");
    let mut empty_field =
        MessageBag::try_from_messages([(key("field"), MessageInput::Many(Collection::default()))])
            .expect("empty field");
    empty_field
        .merge(
            &Collection::try_from_entries([(key("field"), Collection::default())])
                .expect("incoming"),
        )
        .expect("empty field merge");
    for (name, candidate, field) in [
        ("outer_sparse_merge", outer, "2"),
        ("outer_sparse_merge_empty", outer_empty, "2"),
        ("inner_sparse_merge", inner, "field"),
        ("inner_sparse_merge_empty", inner_empty, "field"),
        ("empty_field_merge_empty", empty_field, "field"),
    ] {
        compare(&format!("{name}.raw"), candidate.to_array().to_value());
        compare(
            &format!("{name}.json"),
            candidate
                .to_json(MessageJsonOptions::NONE)
                .map(|value| json!(value)),
        );
        compare(
            &format!("{name}.get"),
            candidate.get(field, None).and_then(|value| match value {
                MessageLookup::Exact(values) => values.to_value(),
                MessageLookup::Wildcard(values) => values.to_value(),
                MessageLookup::Missing => Ok(json!([])),
            }),
        );
        compare(
            &format!("{name}.all"),
            candidate.all(None).and_then(|value| value.to_value()),
        );
        compare(&format!("{name}.count"), Ok(json!(candidate.count())));
    }
    let mut nested = MessageBag::try_from_messages([(
        key("field"),
        MessageInput::Many(
            Collection::try_from_entries([(key("label"), "x".into())]).expect("label"),
        ),
    )])
    .expect("nested");
    nested
        .merge(
            &Collection::try_from_entries([(
                key("field"),
                Collection::try_from_entries([(key("label"), MessageValue::from("y"))])
                    .expect("incoming label"),
            )])
            .expect("incoming"),
        )
        .expect("nested merge");
    for (label, requested, default) in [
        ("default", None, ":message"),
        ("key_message", Some(":key: :message"), ":message"),
        ("message_bang", Some(":message!"), ":message"),
        ("empty_fallback", Some(""), "[:key] :message"),
        ("zero_fallback", Some("0"), "[:key] :message"),
    ] {
        nested.set_format(default);
        let prefix = format!("nested.{label}");
        compare(
            &format!("{prefix}.get"),
            nested
                .get("field", requested)
                .and_then(|value| match value {
                    MessageLookup::Exact(values) => values.to_value(),
                    MessageLookup::Wildcard(values) => values.to_value(),
                    MessageLookup::Missing => Ok(json!([])),
                }),
        );
        compare(
            &format!("{prefix}.first"),
            nested
                .first(Some("field"), requested)
                .map(|value| json!(value)),
        );
        compare(
            &format!("{prefix}.all"),
            nested.all(requested).and_then(|value| value.to_value()),
        );
        compare(
            &format!("{prefix}.unique"),
            nested.unique(requested).and_then(|value| value.to_value()),
        );
    }
    for plain in &bare {
        let name = plain["case"].as_str().expect("case");
        let framework = managed
            .iter()
            .find(|row| row["case"] == name)
            .expect("paired case");
        if name.starts_with("nested.") && !name.starts_with("nested.default.") {
            assert!(
                plain["result"].get("value").is_some()
                    || plain["result"]["exception"] == "ArgumentCountError"
            );
            assert_eq!(framework["result"]["exception"], "ErrorException");
        } else {
            assert_eq!(plain["result"], framework["result"]);
        }
    }
}

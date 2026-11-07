//! 固定 Passport Scope、BridgeScopeRepository、Client 与 Token 的纯规则回归。

use super::{Scope, ScopeRepository};
use crate::{Client, ClientModel, PhpArray};
use openrust::{
    Error, Result,
    database::eloquent::{ModelIdentity, ModelMetadata},
    hashing::HashManager,
    support::BoxFuture,
};
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex};
fn array(values: &[&str]) -> PhpArray {
    strings(values).into()
}

fn scope(id: &str, description: &str) -> Scope {
    Scope {
        id: id.to_owned(),
        description: description.to_owned(),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn heterogeneous_membership_preserves_php_types_and_associative_values() -> openrust::Result<()> {
    use serde_json::{Value, json};
    let repository = ScopeRepository::default();
    for (needle, value, expected) in [
        ("01", json!(1), true),
        ("anything", json!(true), true),
        ("0", json!(false), true),
        ("", Value::Null, true),
        ("0", Value::Null, false),
        ("text", json!(0), false),
        ("text", json!([]), false),
        ("text", json!({}), false),
    ] {
        let haystack = PhpArray::try_from(json!({"key":value}))?;
        assert_eq!(
            repository.client_has_scope(Some(&haystack), needle),
            expected
        );
    }
    let granted = PhpArray::try_from(json!({"allow":true,"retain":null}))?;
    assert!(repository.token_can(&granted, "orders:read"));
    let inherited = repository.with_inherited_scopes(true);
    let granted = PhpArray::try_from(json!({"numeric":1,"tail":null}))?;
    assert!(inherited.client_has_scope(Some(&granted), "01:child"));
    assert_eq!(
        serde_json::to_value(&granted).map_err(openrust::Error::internal)?,
        json!({"numeric":1,"tail":null})
    );
    Ok(())
}

#[test]
fn scope_json_contains_only_identifier_and_description() -> Result<(), serde_json::Error> {
    let expected = scope("orders:read", "读取订单");
    let json = serde_json::to_value(&expected)?;
    assert_eq!(
        json,
        serde_json::json!({"id":"orders:read","description":"读取订单"})
    );
    assert_eq!(serde_json::from_value::<Scope>(json)?, expected);
    Ok(())
}

#[test]
fn default_is_empty_and_wildcard_is_not_an_implicit_registered_scope() {
    let repository = ScopeRepository::default();
    assert_eq!(repository.scope_ids(), Vec::<String>::new());
    assert_eq!(repository.scopes(), Vec::<Scope>::new());
    assert!(repository.has_scope("*"));
    assert!(!repository.has_scope("orders"));
    assert_eq!(
        repository.scopes_for(&strings(&["*", "unknown"])),
        Vec::<Scope>::new()
    );
    assert!(!repository.token_can(&array(&["orders"]), "orders:read"));
}

#[test]
fn duplicate_definitions_replace_value_at_first_position_and_tokens_can_replaces_all() {
    let mut repository = ScopeRepository::new([
        scope("second", "旧说明"),
        scope("first", "第一版"),
        scope("second", "新说明"),
    ])
    .with_inherited_scopes(true);
    assert_eq!(repository.scope_ids(), strings(&["second", "first"]));
    assert_eq!(
        repository.scopes(),
        vec![scope("second", "新说明"), scope("first", "第一版")]
    );
    repository.tokens_can([scope("replacement", "替换"), scope("replacement", "最终")]);
    assert_eq!(repository.scopes(), vec![scope("replacement", "最终")]);
    assert!(!repository.has_scope("second"));
    assert!(repository.token_can(&array(&["replacement"]), "replacement:child"));
}

#[test]
fn scopes_for_retains_input_order_duplicates_and_only_registered_wildcard() {
    let repository = ScopeRepository::new([scope("a", "A"), scope("b", "B")]);
    assert_eq!(
        repository.scopes_for(&strings(&["b", "a", "b", "unknown", "*"])),
        vec![scope("b", "B"), scope("a", "A"), scope("b", "B")]
    );
    let registered = ScopeRepository::new([scope("*", "全局")]);
    assert_eq!(registered.scope_ids(), strings(&["*"]));
    assert_eq!(
        registered.scopes_for(&strings(&["*", "*"])),
        vec![scope("*", "全局"), scope("*", "全局")]
    );
}

#[test]
fn client_missing_null_empty_and_wildcard_restrictions_are_distinct() {
    let repository = ScopeRepository::default();
    assert!(repository.client_has_scope(None, "unknown"));
    assert!(repository.client_has_scope(None, "*"));
    assert!(!repository.client_has_scope(Some(&PhpArray::default()), "orders"));
    assert!(!repository.client_has_scope(Some(&PhpArray::default()), "*"));
    let restrictions = array(&["orders", "*"]);
    assert!(repository.client_has_scope(Some(&restrictions), "orders"));
    assert!(repository.client_has_scope(Some(&restrictions), "*"));
    assert!(!repository.client_has_scope(Some(&restrictions), "other"));
    assert!(!repository.client_has_scope(Some(&restrictions), "orders:read"));
}

#[test]
fn token_empty_is_denied_and_wildcard_is_universal() {
    let repository = ScopeRepository::default();
    assert!(!repository.token_can(&PhpArray::default(), "*"));
    assert!(!repository.token_can(&PhpArray::default(), "orders"));
    assert!(repository.token_can(&array(&["*"]), "orders:read"));
    assert!(repository.token_can(&array(&["orders"]), "orders"));
    assert!(!repository.token_can(&array(&["orders"]), "orders:read"));
}

#[test]
fn inherited_scopes_match_colon_ancestors_without_reverse_or_partial_prefix_grants() {
    let repository = ScopeRepository::default().with_inherited_scopes(true);
    let granted = array(&["orders", "admin:webhooks:read"]);
    assert!(repository.token_can(&granted, "orders:read:limited"));
    assert!(repository.client_has_scope(Some(&granted), "orders:write"));
    assert!(repository.token_can(&granted, "admin:webhooks:read:limited"));
    assert!(!repository.token_can(&granted, "admin:webhooks"));
    assert!(!repository.token_can(&granted, "ordersarchive:read"));
    assert!(!repository.token_can(&array(&["orders:read"]), "orders"));
    assert!(
        !repository
            .with_inherited_scopes(false)
            .token_can(&granted, "orders:read")
    );
}

#[test]
fn inherited_matching_preserves_empty_colon_segments_and_unicode_identifiers() {
    let repository = ScopeRepository::default().with_inherited_scopes(true);
    assert!(repository.token_can(&array(&[""]), ":read"));
    assert!(repository.token_can(&array(&["a:"]), "a::read"));
    assert!(repository.token_can(&array(&["订单"]), "订单:读取"));
    assert!(!repository.token_can(&array(&["订"]), "订单:读取"));
    // 客户端星号仍只是普通父标识符；仅冒号继承能匹配它的命名空间。
    assert!(repository.client_has_scope(Some(&array(&["*"])), "*:read"));
    assert!(!repository.client_has_scope(Some(&array(&["*"])), "orders:read"));
}

#[test]
fn finalize_filters_grant_wildcard_before_global_and_client_rules() {
    let repository = ScopeRepository::new([scope("a", "A"), scope("b", "B")]);
    let requested = strings(&["*", "b", "unknown", "a", "b", "*"]);
    for grant in [
        "authorization_code",
        "refresh_token",
        "implicit",
        "urn:ietf:params:oauth:grant-type:device_code",
        "unknown",
        "PASSWORD",
    ] {
        assert_eq!(
            repository.finalize_scopes(&requested, grant, None),
            strings(&["b", "a", "b"])
        );
    }
    for grant in ["password", "personal_access", "client_credentials"] {
        assert_eq!(
            repository.finalize_scopes(&requested, grant, None),
            strings(&["*", "b", "a", "b", "*"])
        );
        assert_eq!(
            repository.finalize_scopes(&requested, grant, Some(&array(&["a"]))),
            strings(&["a"])
        );
    }
}

#[test]
fn finalize_preserves_duplicates_but_does_not_invent_global_inherited_scopes() {
    let repository = ScopeRepository::new([
        scope("orders", "订单"),
        scope("orders:read", "读取"),
        scope("other", "其他"),
    ])
    .with_inherited_scopes(true);
    let requested = strings(&["orders:read", "orders:unknown", "other", "orders:read"]);
    assert_eq!(
        repository.finalize_scopes(&requested, "authorization_code", Some(&array(&["orders"]))),
        strings(&["orders:read", "orders:read"])
    );
    assert_eq!(
        repository.finalize_scopes(&requested, "authorization_code", Some(&PhpArray::default())),
        Vec::<String>::new()
    );
    assert_eq!(
        repository.finalize_scopes(&requested, "authorization_code", None),
        strings(&["orders:read", "other", "orders:read"])
    );
}

#[test]
fn client_and_token_scope_membership_uses_php_loose_numeric_string_comparison() {
    let repository = ScopeRepository::default();
    for (granted, requested) in [("01", "1"), ("1e3", "1000"), (" \t+1.0\n", "1")] {
        assert!(repository.client_has_scope(Some(&array(&[granted])), requested));
        assert!(repository.token_can(&array(&[granted]), requested));
        assert!(repository.client_has_scope(Some(&array(&[requested])), granted));
        assert!(repository.token_can(&array(&[requested]), granted));
    }
    let inherited = repository.with_inherited_scopes(true);
    assert!(inherited.client_has_scope(Some(&array(&["01"])), "1:read"));
    assert!(inherited.token_can(&array(&["1e3"]), "1000:read"));
}

#[test]
fn global_registry_keys_remain_exact_while_final_client_filter_is_loose() {
    let repository = ScopeRepository::new([
        scope("01", "前导零"),
        scope("1", "整数"),
        scope("1e3", "指数"),
    ]);
    assert_eq!(repository.scope_ids(), strings(&["01", "1", "1e3"]));
    assert!(!repository.has_scope("1000"));
    assert_eq!(
        repository.scopes_for(&strings(&["1000", "1", "01"])),
        vec![scope("1", "整数"), scope("01", "前导零")]
    );
    assert_eq!(
        repository.finalize_scopes(
            &strings(&["01", "1000", "1e3", "1", "01"]),
            "authorization_code",
            Some(&array(&["1", "1000"]))
        ),
        strings(&["01", "1e3", "1", "01"])
    );
}

#[test]
fn numeric_scope_comparison_preserves_overflow_fallback_and_non_numeric_identity() {
    let repository = ScopeRepository::default();
    for (granted, requested) in [
        ("9007199254740992", "9007199254740993"),
        ("9223372036854775808", "9223372036854775809"),
        ("-9223372036854775809", "-9223372036854775810"),
        ("9223372036854775807", "9223372036854775808"),
        ("1e9999", "2e9999"),
        ("-1e9999", "-2e9999"),
        ("1e9999", "INF"),
        ("1suffix", "1"),
        ("１", "1"),
        ("订单", "订"),
    ] {
        assert!(
            !repository.client_has_scope(Some(&array(&[granted])), requested),
            "{granted:?}, {requested:?}"
        );
        assert!(!repository.token_can(&array(&[granted]), requested));
        assert!(!repository.token_can(&array(&[requested]), granted));
    }
    for same in ["9223372036854775808", "1e9999", "-1e9999", "订单"] {
        assert!(repository.client_has_scope(Some(&array(&[same])), same));
        assert!(repository.token_can(&array(&[same]), same));
    }
}

struct DispatchClient {
    model: Client,
    calls: Mutex<Vec<String>>,
    denied: Vec<String>,
    fail_on: Option<String>,
}

impl DispatchClient {
    fn new(denied: &[&str], fail_on: Option<&str>) -> Self {
        let model = Client::from_attributes(
            Arc::new(ModelMetadata::new(
                "oauth_clients",
                "fixture::DispatchClient",
            )),
            Map::from_iter([
                ("id".into(), Value::String("dispatch-client".into())),
                ("scopes".into(), Value::Null),
            ]),
        );
        Self {
            model,
            calls: Mutex::new(Vec::new()),
            denied: strings(denied),
            fail_on: fail_on.map(str::to_owned),
        }
    }

    fn calls(&self) -> Result<Vec<String>> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .map_err(|_| dispatch_lock_error())
    }
}

fn dispatch_lock_error() -> Error {
    Error::custom(
        "scope_dispatch_fixture_lock_poisoned",
        "scope派发夹具锁不可用",
    )
}

impl ModelIdentity for DispatchClient {
    fn model_metadata(&self) -> &ModelMetadata {
        self.model.model_metadata()
    }
    fn model_key(&self) -> Result<Value> {
        self.model.model_key()
    }
}

impl ClientModel for DispatchClient {
    fn attributes(&self) -> Result<Map<String, Value>> {
        self.model.attributes()
    }
    fn get_attribute(&self, name: &str) -> Result<Option<Value>> {
        self.model.get_attribute(name)
    }
    fn set_raw_attributes(&self, attributes: Map<String, Value>) -> Result<()> {
        self.model.set_raw_attributes(attributes)
    }
    fn plain_secret(&self) -> Result<Option<String>> {
        self.model.plain_secret()
    }
    fn set_secret<'a>(
        &'a self,
        hashing: &'a HashManager,
        secret: Option<&'a str>,
    ) -> BoxFuture<'a, Result<()>> {
        self.model.set_secret(hashing, secret)
    }
    fn has_scope(&self, _scopes: &ScopeRepository, scope: &str) -> Result<bool> {
        self.calls
            .lock()
            .map_err(|_| dispatch_lock_error())?
            .push(scope.to_owned());
        if self.fail_on.as_deref() == Some(scope) {
            return Err(Error::custom(
                "scope_fixture_policy_unavailable",
                "实际模型scope策略不可用",
            ));
        }
        Ok(!self.denied.iter().any(|denied| denied == scope))
    }
}

#[test]
fn model_dispatch_honors_override_denial_even_when_raw_restrictions_are_null() -> Result<()> {
    let repository = ScopeRepository::new([scope("a", "A"), scope("b", "B")]);
    let client = DispatchClient::new(&["a"], None);
    let requested = strings(&["a", "b", "a"]);
    assert_eq!(client.attributes()?.get("scopes"), Some(&Value::Null));
    assert_eq!(
        repository.finalize_scopes(&requested, "authorization_code", None),
        requested
    );
    assert_eq!(
        repository.finalize_model_scopes(&requested, "authorization_code", Some(&client))?,
        strings(&["b"])
    );
    assert_eq!(client.calls()?, requested);
    Ok(())
}

#[test]
fn model_dispatch_calls_only_grant_and_global_survivors_in_order_with_duplicates() -> Result<()> {
    let repository = ScopeRepository::new([scope("a", "A"), scope("b", "B")]);
    let client = DispatchClient::new(&[], None);
    let requested = strings(&["*", "b", "unknown", "a", "b", "*"]);
    assert_eq!(
        repository.finalize_model_scopes(&requested, "authorization_code", Some(&client))?,
        strings(&["b", "a", "b"])
    );
    assert_eq!(client.calls()?, strings(&["b", "a", "b"]));
    assert_eq!(
        repository.finalize_model_scopes(&requested, "password", Some(&client))?,
        strings(&["*", "b", "a", "b", "*"])
    );
    assert_eq!(
        client.calls()?,
        strings(&["b", "a", "b", "*", "b", "a", "b", "*"])
    );
    Ok(())
}

#[test]
fn model_dispatch_returns_original_error_and_stops_later_callbacks() -> Result<()> {
    let repository = ScopeRepository::new([scope("a", "A"), scope("b", "B"), scope("c", "C")]);
    let client = DispatchClient::new(&[], Some("b"));
    let error = repository
        .finalize_model_scopes(
            &strings(&["unknown", "a", "b", "c", "b"]),
            "refresh_token",
            Some(&client),
        )
        .err()
        .ok_or_else(|| Error::custom("scope_fixture_expected_error", "模型错误未透传"))?;
    assert_eq!(error.code(), "scope_fixture_policy_unavailable");
    assert_eq!(client.calls()?, strings(&["a", "b"]));
    Ok(())
}

#[test]
fn absent_model_skips_only_client_filter_and_does_not_assert_active_identity() -> Result<()> {
    let repository = ScopeRepository::new([scope("b", "B")]);
    let requested = strings(&["*", "unknown", "b", "b"]);
    for grant in ["authorization_code", "password"] {
        assert_eq!(
            repository.finalize_model_scopes(&requested, grant, None)?,
            repository.finalize_scopes(&requested, grant, None)
        );
    }
    assert_eq!(
        repository.finalize_model_scopes(&requested, "authorization_code", None)?,
        strings(&["b", "b"])
    );
    Ok(())
}

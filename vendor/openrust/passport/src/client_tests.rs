//! 固定 Passport Client 属性、secret 生命周期和替换工厂回归。
use super::*;
use openrust::hashing::{BcryptConfig, HashOptions, HashingConfig};
use serde_json::json;

fn attributes(value: Value) -> Result<Map<String, Value>> {
    match value {
        Value::Object(attributes) => Ok(attributes),
        _ => Err(invalid_attribute()),
    }
}
fn client(value: Value) -> Result<Arc<dyn ClientModel>> {
    ClientFactory::default().new_from_builder(attributes(value)?)
}
fn hashing() -> Result<HashManager> {
    HashManager::new(HashingConfig {
        bcrypt: BcryptConfig {
            rounds: 4,
            verify: true,
            limit: None,
        },
        ..Default::default()
    })
}

#[test]
fn redirects_use_nonempty_json_before_untrimmed_legacy_and_preserve_order() -> Result<()> {
    assert_eq!(
        client(json!({"redirect_uris":"[\"b\",\"a\"]","redirect":"old"}))?
            .redirect_uris()?
            .as_value(),
        &json!(["b", "a"])
    );
    assert_eq!(
        client(json!({"redirect_uris":"[]","redirect":"old"}))?
            .redirect_uris()?
            .as_value(),
        &json!([])
    );
    for value in [Value::Null, json!(""), json!("0")] {
        assert_eq!(
            client(json!({"redirect_uris":value,"redirect":"a, b,,c"}))?
                .redirect_uris()?
                .as_value(),
            &json!(["a", " b", "", "c"])
        );
    }
    assert_eq!(client(json!({}))?.redirect_uris()?.as_value(), &json!([]));
    assert_eq!(
        client(json!({"redirect":true}))?
            .redirect_uris()?
            .as_value(),
        &json!(["1"])
    );
    assert_eq!(
        client(json!({"redirect":42}))?.redirect_uris()?.as_value(),
        &json!(["42"])
    );
    assert!(
        client(json!({"redirect_uris":"broken"}))?
            .redirect_uris()
            .is_err()
    );
    Ok(())
}

#[test]
fn explicit_grants_even_empty_precede_legacy_seven_grant_order() -> Result<()> {
    let model = client(
        json!({"redirect_uris":"[\"https://app.test/callback\"]", "secret":"hash", "password_client":1,"personal_access_client":1}),
    )?;
    assert_eq!(
        model.grant_types()?.as_value(),
        &json!([
            "authorization_code",
            "client_credentials",
            "implicit",
            "password",
            "personal_access",
            "refresh_token",
            "urn:ietf:params:oauth:grant-type:device_code"
        ])
    );
    assert!(model.has_grant_type("authorization_code")?);
    assert!(!model.has_grant_type("unknown")?);
    for grants in [json!("[]"), json!([])] {
        assert_eq!(
            client(json!({"grant_types":grants,"redirect":"old","secret":"hash"}))?
                .grant_types()?
                .as_value(),
            &json!([])
        );
    }
    assert_eq!(
        client(json!({"grant_types":null,"owner_id":42}))?
            .grant_types()?
            .as_value(),
        &json!([
            "refresh_token",
            "urn:ietf:params:oauth:grant-type:device_code"
        ])
    );
    Ok(())
}

#[test]
fn first_party_and_confidential_use_php_empty_with_user_id_presence_precedence() -> Result<()> {
    for value in [
        Value::Null,
        json!(false),
        json!(0),
        json!("0"),
        json!(""),
        json!([]),
    ] {
        let model = client(json!({"user_id":value,"owner_id":42,"secret":value}))?;
        assert!(model.first_party()?);
        assert!(!model.confidential()?);
    }
    assert!(!client(json!({"user_id":42,"owner_id":null}))?.first_party()?);
    assert!(!client(json!({"owner_id":"owner"}))?.first_party()?);
    assert!(client(json!({"secret":"false","revoked":"false"}))?.confidential()?);
    assert!(client(json!({"revoked":"false"}))?.revoked()?);
    assert!(!client(json!({"revoked":"0"}))?.revoked()?);
    Ok(())
}

#[test]
fn scope_null_is_unrestricted_but_empty_and_star_do_not_bypass_restrictions() -> Result<()> {
    let scopes = ScopeRepository::default();
    for value in [json!({}), json!({"scopes":null})] {
        assert!(client(value)?.has_scope(&scopes, "anything")?);
    }
    assert!(!client(json!({"scopes":"[]"}))?.has_scope(&scopes, "read")?);
    assert!(!client(json!({"scopes":"[\"*\"]"}))?.has_scope(&scopes, "read")?);
    assert!(client(json!({"scopes":"[\"read\"]"}))?.has_scope(&scopes, "read")?);
    Ok(())
}

#[tokio::test]
async fn secret_assignment_casts_once_null_clears_and_hydration_has_no_plaintext() -> Result<()> {
    let hashing = hashing()?;
    let model = client(json!({"secret":"raw-stored"}))?;
    assert_eq!(model.secret()?.as_deref(), Some("raw-stored"));
    assert_eq!(model.plain_secret()?, None);
    model.set_secret(&hashing, Some("client-secret")).await?;
    assert_eq!(model.plain_secret()?.as_deref(), Some("client-secret"));
    let encoded = model.secret()?.ok_or_else(invalid_attribute)?;
    assert!(
        hashing
            .check("client-secret", Some(&encoded), &HashOptions::new())
            .await?
    );
    model.set_secret(&hashing, Some(&encoded)).await?;
    assert_eq!(model.secret()?.as_deref(), Some(encoded.as_str()));
    assert_eq!(model.plain_secret()?.as_deref(), Some(encoded.as_str()));
    model.set_secret(&hashing, None).await?;
    assert_eq!(model.secret()?, None);
    assert_eq!(model.plain_secret()?, None);
    assert!(!model.confidential()?);
    Ok(())
}

#[tokio::test]
async fn failed_secret_cast_records_plain_input_and_preserves_previous_storage() -> Result<()> {
    let hashing = hashing()?;
    let model = client(json!({"secret":"previous"}))?;
    assert!(model.set_secret(&hashing, Some("a\0b")).await.is_err());
    assert_eq!(model.plain_secret()?.as_deref(), Some("a\0b"));
    assert_eq!(model.secret()?.as_deref(), Some("previous"));
    model.set_raw_attributes(attributes(json!({"secret":"hydrated"}))?)?;
    assert_eq!(model.secret()?.as_deref(), Some("hydrated"));
    // 原始持久回写不重置赋值请求的临时属性。
    assert_eq!(model.plain_secret()?.as_deref(), Some("a\0b"));
    Ok(())
}

#[test]
fn json_hides_both_secrets_and_applies_visibility_array_and_bool_casts() -> Result<()> {
    let mut metadata = ClientFactory::default().metadata().clone();
    metadata.hidden.push("internal".into());
    let model = Client::from_attributes(
        Arc::new(metadata.clone()),
        attributes(json!({
            "id":"id", "secret":"never-output", "plainSecret":"never-output", "plain_secret":"never-output",
            "internal":"hidden", "redirect_uris":"[\"a\",\"b\"]", "grant_types":"[]",
            "scopes":"[\"read\"]", "revoked":1,"password_client":"0", "personal_access_client":null
        }))?,
    );
    let serialized = serde_json::to_value(&model).map_err(Error::internal)?;
    assert_eq!(
        serialized,
        json!({"id":"id","redirect_uris":["a","b"],"grant_types":[],"scopes":["read"],"revoked":true,"password_client":false,"personal_access_client":null})
    );
    metadata.visible = vec!["id".into(), "secret".into()];
    let visible = Client::from_attributes(Arc::new(metadata), model.attributes()?);
    assert_eq!(
        serde_json::to_value(visible).map_err(Error::internal)?,
        json!({"id":"id"})
    );
    Ok(())
}

#[test]
fn default_factory_uuid_format_and_generated_v7_are_explicit() -> Result<()> {
    let factory = ClientFactory::default();
    for value in [
        json!("550e8400-e29b-41d4-a716-446655440000"),
        json!("550E8400-E29B-41D4-A716-446655440000"),
    ] {
        assert!(factory.is_valid_identifier(&value));
    }
    for value in [
        json!(42),
        Value::Null,
        json!("550e8400e29b41d4a716446655440000"),
        json!("urn:uuid:550e8400-e29b-41d4-a716-446655440000"),
        json!("not-uuid"),
    ] {
        assert!(!factory.is_valid_identifier(&value));
    }
    let id = factory.new_unique_id()?;
    assert!(factory.is_valid_identifier(&id));
    let parsed = uuid::Uuid::parse_str(id.as_str().ok_or_else(invalid_attribute)?)
        .map_err(Error::internal)?;
    assert_eq!(parsed.get_version_num(), 7);
    assert_eq!(factory.metadata().table, "oauth_clients");
    assert_eq!(factory.metadata().key_type, "string");
    assert!(!factory.metadata().incrementing);
    Ok(())
}

struct NumericFactory {
    metadata: ModelMetadata,
}
impl ClientModelFactory for NumericFactory {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }
    fn is_valid_identifier(&self, value: &Value) -> bool {
        value.as_i64().is_some_and(|id| id > 0)
    }
    fn new_unique_id(&self) -> Result<Value> {
        Ok(json!(42))
    }
    fn new_from_builder(&self, attributes: Map<String, Value>) -> Result<Arc<dyn ClientModel>> {
        Ok(Arc::new(Client::from_attributes(
            Arc::new(self.metadata.clone()),
            attributes,
        )))
    }
}
#[test]
fn custom_factory_keeps_table_connection_key_name_and_raw_numeric_identity() -> Result<()> {
    let mut metadata = ModelMetadata::new("tenant_clients", "app::Client");
    metadata.connection = Some("tenant".into());
    metadata.key_name = "client_number".into();
    let factory = NumericFactory { metadata };
    assert!(factory.is_valid_identifier(&json!(42)));
    assert!(!factory.is_valid_identifier(&json!("42")));
    let model = factory.new_from_builder(attributes(
        json!({"client_number":factory.new_unique_id()?,"name":"custom","provider":"accounts"}),
    )?)?;
    assert_eq!(model.model_key()?, json!(42));
    assert_eq!(model.morph_class()?, json!("app::Client"));
    assert_eq!(model.model_metadata().table, "tenant_clients");
    assert_eq!(model.model_metadata().connection.as_deref(), Some("tenant"));
    assert_eq!(model.name()?.as_deref(), Some("custom"));
    assert_eq!(model.provider()?.as_deref(), Some("accounts"));
    Ok(())
}

struct TrustedClient(Client);
impl ModelIdentity for TrustedClient {
    fn model_metadata(&self) -> &ModelMetadata {
        self.0.model_metadata()
    }
    fn model_key(&self) -> Result<Value> {
        self.0.model_key()
    }
}
impl ClientModel for TrustedClient {
    fn attributes(&self) -> Result<Map<String, Value>> {
        self.0.attributes()
    }
    fn set_raw_attributes(&self, attributes: Map<String, Value>) -> Result<()> {
        self.0.set_raw_attributes(attributes)
    }
    fn plain_secret(&self) -> Result<Option<String>> {
        self.0.plain_secret()
    }
    fn set_secret<'a>(
        &'a self,
        hashing: &'a HashManager,
        secret: Option<&'a str>,
    ) -> BoxFuture<'a, Result<()>> {
        self.0.set_secret(hashing, secret)
    }
    fn skips_authorization(&self, _user: &dyn Authenticatable, scopes: &[Scope]) -> Result<bool> {
        Ok(scopes.iter().all(|scope| scope.id == "read"))
    }
}
#[test]
fn default_consent_is_required_and_custom_model_can_select_explicit_scopes() -> Result<()> {
    let user = openrust::auth::GenericUser::new(attributes(json!({"id":42}))?);
    let scope = Scope {
        id: "read".into(),
        description: "读取".into(),
    };
    assert!(!client(json!({}))?.skips_authorization(&user, std::slice::from_ref(&scope))?);
    let trusted: Arc<dyn ClientModel> = Arc::new(TrustedClient(Client::from_attributes(
        Arc::new(ClientFactory::default().metadata().clone()),
        Map::new(),
    )));
    assert!(trusted.skips_authorization(&user, &[scope])?);
    assert!(!trusted.skips_authorization(
        &user,
        &[Scope {
            id: "write".into(),
            description: "写入".into()
        }]
    )?);
    Ok(())
}

struct ConstantGetter(Value);
impl AttributeGetter for ConstantGetter {
    fn get(&self, _: Option<&Value>, _: &Map<String, Value>) -> Result<Option<Value>> {
        Ok(Some(self.0.clone()))
    }
}
fn getter(model: &Client, name: &str, value: Value) -> Result<()> {
    model
        .model_attributes()
        .register_getter(name, Arc::new(ConstantGetter(value)))
}
fn concrete_client(value: Value) -> Result<Client> {
    Ok(Client::from_attributes(
        Arc::new(ClientFactory::default().metadata().clone()),
        attributes(value)?,
    ))
}

#[test]
fn registered_getters_reach_client_bridge_and_json_without_mutating_hydrated_raw() -> Result<()> {
    let raw = attributes(json!({
        "id":"00042", "name":"raw name", "provider":"raw provider", "secret":"stored hash",
        "redirect":"raw callback", "revoked":1, "scopes":"invalid JSON"
    }))?;
    let model = Client::from_attributes(
        Arc::new(ClientFactory::default().metadata().clone()),
        raw.clone(),
    );
    for (name, value) in [
        ("id", json!("projected-id")),
        ("name", json!("getter name")),
        ("provider", json!("accounts")),
        ("revoked", json!(false)),
        ("scopes", json!(["read"])),
        ("secret", Value::Null),
        ("redirect_uris", json!(["https://getter.test/callback"])),
        ("grant_types", json!(["authorization_code"])),
    ] {
        getter(&model, name, value)?;
    }
    let bridge = crate::BridgeClient::from_client_model(&model)?;
    assert_eq!(bridge.get_identifier(), "projected-id");
    assert_eq!(bridge.get_name()?, "getter name");
    assert_eq!(bridge.provider.as_deref(), Some("accounts"));
    assert_eq!(
        bridge.get_redirect_uri().as_value(),
        &json!(["https://getter.test/callback"])
    );
    assert_eq!(
        bridge.grant_types.as_value(),
        &json!(["authorization_code"])
    );
    assert!(bridge.is_confidential());
    assert!(model.confidential()?);
    assert_eq!(model.secret()?.as_deref(), Some("stored hash"));
    assert!(!model.revoked()?);
    let scopes = ScopeRepository::default();
    assert!(model.has_scope(&scopes, "read")?);
    assert!(!model.has_scope(&scopes, "write")?);
    assert_eq!(model.attributes()?, raw);
    assert_eq!(model.plain_secret()?, None);
    let serialized = serde_json::to_value(&model).map_err(Error::internal)?;
    assert_eq!(serialized["name"], json!("getter name"));
    assert_eq!(serialized["provider"], json!("accounts"));
    assert_eq!(serialized["revoked"], json!(false));
    assert_eq!(serialized["scopes"], json!(["read"]));
    assert!(serialized.get("secret").is_none());
    Ok(())
}

#[test]
fn inherited_redirect_and_grant_accessors_read_other_fields_through_the_same_pipeline() -> Result<()>
{
    let model = concrete_client(json!({
        "id":"client", "name":"app", "redirect":"raw", "secret":"hash",
        "user_id":null, "owner_id":null, "password_client":true, "personal_access_client":false
    }))?;
    // 固定 Client.php:150–157 从回调 raw attributes 读 legacy redirect，不调用该字段 getter。
    let legacy = Arc::new(CountingLegacyRedirect(std::sync::atomic::AtomicUsize::new(
        0,
    )));
    model
        .model_attributes()
        .register_getter("redirect", legacy.clone())?;
    getter(&model, "user_id", json!("assigned-owner"))?;
    getter(&model, "password_client", json!(false))?;
    getter(&model, "personal_access_client", json!(true))?;
    getter(&model, "secret", Value::Null)?;
    assert_eq!(model.redirect_uris()?.as_value(), &json!(["raw"]));
    assert_eq!(legacy.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(!model.first_party()?);
    assert_eq!(
        model.grant_types()?.as_value(),
        &json!([
            "authorization_code",
            "implicit",
            "personal_access",
            "refresh_token",
            "urn:ietf:params:oauth:grant-type:device_code"
        ])
    );
    // 官方先检查 raw user_id 是否存在；仅其不存在时才改读 owner_id。
    model.set_raw_attributes(attributes(json!({"secret":"hash","owner_id":7}))?)?;
    getter(&model, "owner_id", Value::Null)?;
    assert!(model.first_party()?);
    assert!(model.has_grant_type("client_credentials")?);
    assert_eq!(legacy.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn explicit_empty_getters_override_builtins_while_null_is_not_a_fallback_signal() -> Result<()> {
    let model = concrete_client(json!({"redirect":"legacy", "secret":"hash"}))?;
    getter(&model, "redirect_uris", json!([]))?;
    assert!(model.redirect_uris()?.is_empty());
    assert!(!model.has_grant_type("authorization_code")?);
    assert!(!model.has_grant_type("implicit")?);
    getter(&model, "grant_types", json!([]))?;
    assert!(model.grant_types()?.is_empty());
    for field in ["redirect_uris", "grant_types"] {
        getter(&model, field, Value::Null)?;
        assert_eq!(model.get_attribute(field)?, Some(Value::Null));
        let result = if field == "redirect_uris" {
            model.redirect_uris()
        } else {
            model.grant_types()
        };
        assert!(matches!(result, Err(error)
            if error.code() == "passport_client_attribute_invalid" && error.status().as_u16() == 500));
        // 用户 getter 返回字符串不等于内置访问器读取原始 JSON 文本。
        getter(&model, field, json!("[]"))?;
        let result = if field == "redirect_uris" {
            model.redirect_uris()
        } else {
            model.grant_types()
        };
        assert!(result.is_err());
    }
    Ok(())
}

struct FailingGetter;
struct CountingLegacyRedirect(std::sync::atomic::AtomicUsize);
impl AttributeGetter for CountingLegacyRedirect {
    fn get(&self, _: Option<&Value>, _: &Map<String, Value>) -> Result<Option<Value>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(invalid_attribute())
    }
}
impl AttributeGetter for FailingGetter {
    fn get(&self, _: Option<&Value>, _: &Map<String, Value>) -> Result<Option<Value>> {
        Err(Error::custom_with_status(
            openrust::http::axum::http::StatusCode::CONFLICT,
            "client_getter_fixture",
            "测试访问器失败",
        ))
    }
}

#[test]
fn associative_and_heterogeneous_arrays_survive_model_bridge_and_serialization() -> Result<()> {
    let model = concrete_client(json!({
        "id":"client", "name":"app", "redirect_uris":"{\"main\":\"https://app.test/cb\",\"flag\":true}",
        "grant_types":"{\"only\":\"authorization_code\",\"number\":1,\"none\":null}",
        "scopes":"[\"read\",null]"
    }))?;
    assert!(model.has_grant_type("authorization_code")?);
    assert!(model.has_grant_type("01")?);
    assert!(model.has_scope(&ScopeRepository::default(), "read")?);
    let bridge = crate::BridgeClient::from_client_model(&model)?;
    let redirect = json!({"main":"https://app.test/cb","flag":true});
    assert_eq!(bridge.get_redirect_uri().as_value(), &redirect);
    assert_eq!(
        bridge
            .get_redirect_uri()
            .values()
            .cloned()
            .collect::<Vec<_>>(),
        vec![json!("https://app.test/cb"), json!(true)]
    );
    assert!(
        bridge
            .get_redirect_uri()
            .contains_strict_string("https://app.test/cb")
    );
    assert!(!bridge.get_redirect_uri().contains_strict_string("anything"));
    assert!(bridge.supports_grant_type("01"));
    let serialized = serde_json::to_value(&model).map_err(Error::internal)?;
    assert_eq!(serialized["redirect_uris"], redirect);
    assert_eq!(
        serialized["grant_types"],
        json!({"only":"authorization_code","number":1,"none":null})
    );
    let roundtrip: crate::PhpArray =
        serde_json::from_value(serialized["grant_types"].clone()).map_err(Error::internal)?;
    assert_eq!(roundtrip.as_value(), &serialized["grant_types"]);
    for scalar in [Value::Null, json!(true), json!(1), json!("[]")] {
        assert!(serde_json::from_value::<crate::PhpArray>(scalar).is_err());
    }
    // 自定义访问器直接返回 PHP 数组载体，不按内置 raw JSON 文本再解析。
    getter(&model, "redirect_uris", json!({"second":2,"first":"1"}))?;
    getter(
        &model,
        "grant_types",
        json!([true, null, 1, "read", ["nested"]]),
    )?;
    let projected = crate::BridgeClient::from_client_model(&model)?;
    assert_eq!(
        projected.get_redirect_uri().as_value(),
        &json!({"second":2,"first":"1"})
    );
    assert!(!projected.get_redirect_uri().contains_strict_string("2"));
    assert!(projected.get_redirect_uri().contains_strict_string("1"));
    assert_eq!(
        projected.grant_types.as_value(),
        &json!([true, null, 1, "read", ["nested"]])
    );
    assert!(projected.supports_grant_type("anything"));
    Ok(())
}

#[tokio::test]
async fn plain_secret_normal_attribute_reads_the_temporary_default_getter_without_leaking()
-> Result<()> {
    let model =
        concrete_client(json!({"id":"client","plain_secret":"spoof","plainSecret":"spoof"}))?;
    assert_eq!(model.plain_secret()?, None);
    assert_eq!(model.get_attribute("plain_secret")?, Some(Value::Null));
    model.set_secret(&hashing()?, Some("real-input")).await?;
    assert_eq!(model.plain_secret()?.as_deref(), Some("real-input"));

    assert_eq!(
        model.get_attribute("plain_secret")?,
        Some(json!("real-input"))
    );
    getter(&model, "plain_secret", json!("registered"))?;
    assert_eq!(
        model.get_attribute("plain_secret")?,
        Some(json!("registered"))
    );
    assert_eq!(model.plain_secret()?.as_deref(), Some("real-input"));
    model
        .model_attributes()
        .register_getter("plain_secret", Arc::new(FailingGetter))?;
    assert!(matches!(model.get_attribute("plain_secret"), Err(error)
        if error.code() == "client_getter_fixture" && error.status().as_u16() == 409));
    let serialized = serde_json::to_value(&model).map_err(Error::internal)?;
    assert!(serialized.get("plain_secret").is_none());
    assert!(serialized.get("plainSecret").is_none());
    assert!(serialized.get("secret").is_none());
    Ok(())
}

#[test]
fn scalar_protocol_getters_are_nullable_and_never_accept_composite_values() -> Result<()> {
    let model = concrete_client(json!({"id":"raw","name":"raw"}))?;
    getter(&model, "id", json!(1.25))?;
    getter(&model, "name", json!(42))?;
    getter(&model, "provider", json!(false))?;
    let bridge = crate::BridgeClient::from_client_model(&model)?;
    assert_eq!(bridge.get_identifier(), "1.25");
    assert_eq!(bridge.get_name()?, "42");
    assert_eq!(bridge.provider.as_deref(), Some(""));
    getter(&model, "id", json!(true))?;
    assert_eq!(
        crate::BridgeClient::from_client_model(&model)?.get_identifier(),
        "1"
    );
    getter(&model, "name", Value::Null)?;
    getter(&model, "provider", Value::Null)?;
    assert_eq!(model.name()?, None);
    assert_eq!(model.provider()?, None);
    for field in ["name", "provider"] {
        getter(&model, field, json!([]))?;
        assert!(crate::BridgeClient::from_client_model(&model).is_err());
        getter(&model, field, Value::Null)?;
    }
    Ok(())
}
#[test]
fn getter_errors_propagate_through_client_and_bridge_without_replacing_raw() -> Result<()> {
    let model = concrete_client(json!({"id":"client", "name":"app", "scopes":"[\"read\"]"}))?;
    let raw = model.attributes()?;
    for field in ["name", "provider", "redirect_uris", "grant_types"] {
        model
            .model_attributes()
            .register_getter(field, Arc::new(FailingGetter))?;
        assert!(
            matches!(crate::BridgeClient::from_client_model(&model), Err(error)
            if error.code() == "client_getter_fixture" && error.status().as_u16() == 409)
        );
        model.model_attributes().register_getter(
            field,
            Arc::new(ConstantGetter(match field {
                "redirect_uris" | "grant_types" => json!([]),
                _ => json!("fixed"),
            })),
        )?;
    }
    model
        .model_attributes()
        .register_getter("revoked", Arc::new(FailingGetter))?;
    assert!(matches!(model.revoked(), Err(error) if error.code() == "client_getter_fixture"));
    model
        .model_attributes()
        .register_getter("scopes", Arc::new(FailingGetter))?;
    assert!(
        matches!(model.has_scope(&ScopeRepository::default(), "read"), Err(error)
        if error.code() == "client_getter_fixture")
    );
    assert_eq!(model.attributes()?, raw);
    Ok(())
}

#[test]
fn client_metadata_casts_and_external_default_read_hook_keep_one_model_identity() -> Result<()> {
    let mut metadata = ClientFactory::default().metadata().clone();
    metadata.casts.insert("name".into(), AttributeCast::String);
    let model = Client::from_attributes(
        Arc::new(metadata),
        attributes(json!({
            "id":"00042", "name":42, "scopes":"[\"read\"]", "revoked":"0", "redirect":"a"
        }))?,
    );
    assert_eq!(model.name()?.as_deref(), Some("42"));
    assert!(!model.revoked()?);
    assert_eq!(model.model_key()?, json!("00042"));
    // 既有可替换模型不必实现新增钩子，仍使用自身元数据和原始快照。
    let external = TrustedClient(model);
    assert_eq!(external.name()?.as_deref(), Some("42"));
    assert_eq!(external.redirect_uris()?.as_value(), &json!(["a"]));
    assert!(external.has_scope(&ScopeRepository::default(), "read")?);
    assert_eq!(external.model_key()?, json!("00042"));
    Ok(())
}

struct AlternatingRedirectGetter(std::sync::atomic::AtomicUsize);
impl AttributeGetter for AlternatingRedirectGetter {
    fn get(&self, _: Option<&Value>, _: &Map<String, Value>) -> Result<Option<Value>> {
        let call = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Some(if call == 0 {
            json!(["first"])
        } else {
            json!([])
        }))
    }
}
#[test]
fn legacy_grant_conditions_keep_official_repeated_property_reads_and_raw_scope_presence()
-> Result<()> {
    let model = concrete_client(json!({}))?;
    let redirects = Arc::new(AlternatingRedirectGetter(
        std::sync::atomic::AtomicUsize::new(0),
    ));
    model
        .model_attributes()
        .register_getter("redirect_uris", redirects.clone())?;
    let grants = model.grant_types()?;
    assert!(grants.contains_loose("authorization_code"));
    assert!(!grants.contains_loose("implicit"));
    assert_eq!(redirects.0.load(std::sync::atomic::Ordering::SeqCst), 2);
    model
        .model_attributes()
        .register_getter("scopes", Arc::new(FailingGetter))?;
    assert!(model.has_scope(&ScopeRepository::default(), "read")?);
    model
        .model_attributes()
        .set_raw_attribute("scopes", Value::Null)?;
    assert!(model.has_scope(&ScopeRepository::default(), "read")?);
    model
        .model_attributes()
        .set_raw_attribute("scopes", json!("[]"))?;
    assert!(
        model
            .has_scope(&ScopeRepository::default(), "read")
            .is_err()
    );
    Ok(())
}

#[test]
fn model_array_serialization_uses_php_list_shape_for_scopes_and_preserves_associative_keys()
-> Result<()> {
    for (raw, expected) in [
        (json!("{}"), json!([])),
        (json!("{\"0\":\"read\"}"), json!(["read"])),
        (json!("{\"1\":\"read\"}"), json!({"1":"read"})),
        (json!("{\"01\":\"read\"}"), json!({"01":"read"})),
        (Value::Null, Value::Null),
    ] {
        let model = client(json!({"id":"array-client", "scopes":raw}))?;
        assert_eq!(model.arrayable_attributes()?.get("scopes"), Some(&expected));
    }
    Ok(())
}

#[test]
fn explicit_empty_redirect_arrays_shadow_legacy_in_model_and_bridge() -> Result<()> {
    let legacy = "https://legacy.test/callback";
    for value in [json!("[]"), json!("{}"), json!([]), json!({})] {
        let model = concrete_client(json!({
            "id":"client", "name":"app", "redirect_uris":value, "redirect":legacy
        }))?;
        assert!(model.redirect_uris()?.is_empty());
        assert!(!model.redirect_uris()?.contains_strict_string(legacy));
        assert!(!model.has_grant_type("authorization_code")?);
        let bridge = crate::BridgeClient::from_client_model(&model)?;
        assert!(bridge.get_redirect_uri().is_empty());
        assert!(!bridge.get_redirect_uri().contains_strict_string(legacy));
    }
    Ok(())
}

#[test]
fn raw_empty_redirect_scalars_keep_official_legacy_fallback_in_model_and_bridge() -> Result<()> {
    let legacy = "https://legacy.test/callback";
    for value in [Value::Null, json!(""), json!("0")] {
        let model = concrete_client(json!({
            "id":"client", "name":"app", "redirect_uris":value, "redirect":legacy
        }))?;
        assert_eq!(model.redirect_uris()?.as_value(), &json!([legacy]));
        let bridge = crate::BridgeClient::from_client_model(&model)?;
        assert!(bridge.get_redirect_uri().contains_strict_string(legacy));
    }
    Ok(())
}

struct SecretOverride {
    fail: bool,
    seen: std::sync::Mutex<Vec<Value>>,
}
impl AttributeSetter for SecretOverride {
    fn set(
        &self,
        value: Value,
        _snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async move {
            self.seen
                .lock()
                .map_err(|_| state_error())?
                .push(value.clone());
            if self.fail {
                return Err(Error::custom(
                    "client_fixture_custom_setter_failed",
                    "合成custom setter拒绝",
                ));
            }
            Ok(Map::from_iter([
                ("secret".to_owned(), value),
                ("custom_applied".to_owned(), Value::Bool(true)),
            ]))
        })
    }
}
fn driver_probe() -> Result<(HashManager, Arc<std::sync::atomic::AtomicUsize>)> {
    let hashing = HashManager::new(HashingConfig {
        driver: "client-setter-probe".to_owned(),
        ..Default::default()
    })?;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let entered = calls.clone();
    hashing.extend("client-setter-probe", move || {
        entered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(Error::custom(
            "client_fixture_hash_driver_entered",
            "合成默认驱动构造失败",
        ))
    })?;
    Ok((hashing, calls))
}
fn assert_no_driver(calls: &std::sync::atomic::AtomicUsize) {
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn public_custom_secret_setter_receives_original_value_without_default_plaintext_or_hashing()
-> Result<()> {
    let model = concrete_client(json!({"secret":"hydrated"}))?;
    model
        .set_secret(&hashing()?, Some("prior-default-input"))
        .await?;
    let setter = Arc::new(SecretOverride {
        fail: false,
        seen: std::sync::Mutex::new(Vec::new()),
    });
    model
        .model_attributes()
        .register_setter("secret", setter.clone())?;
    let (probe, calls) = driver_probe()?;
    model
        .set_secret(&probe, Some("original\0custom-input"))
        .await?;
    assert_eq!(
        model.attributes()?.get("secret"),
        Some(&json!("original\0custom-input"))
    );
    assert_eq!(
        model.attributes()?.get("custom_applied"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        model.plain_secret()?.as_deref(),
        Some("prior-default-input")
    );
    assert_no_driver(&calls);
    model.set_secret(&probe, None).await?;
    assert_eq!(model.secret()?, None);
    assert_eq!(
        model.plain_secret()?.as_deref(),
        Some("prior-default-input")
    );
    assert_eq!(
        *setter.seen.lock().map_err(|_| state_error())?,
        vec![json!("original\0custom-input"), Value::Null]
    );
    assert_no_driver(&calls);
    Ok(())
}

#[tokio::test]
async fn public_failing_custom_secret_setter_preserves_default_plaintext_and_raw_secret()
-> Result<()> {
    let model = concrete_client(json!({"secret":"hydrated"}))?;
    model
        .set_secret(&hashing()?, Some("prior-default-input"))
        .await?;
    let previous = model.attributes()?;
    let setter = Arc::new(SecretOverride {
        fail: true,
        seen: std::sync::Mutex::new(Vec::new()),
    });
    model
        .model_attributes()
        .register_setter("secret", setter.clone())?;
    let (probe, calls) = driver_probe()?;
    for input in [Some("original\0custom-input"), None] {
        assert!(
            matches!(model.set_secret(&probe,input).await,Err(error) if error.code()=="client_fixture_custom_setter_failed")
        );
        assert_eq!(model.attributes()?, previous);
        assert_eq!(
            model.plain_secret()?.as_deref(),
            Some("prior-default-input")
        );
        assert_no_driver(&calls);
    }
    // 记录是custom自身外部副作用，不把它误称为框架可回滚的默认属性状态。
    assert_eq!(
        *setter.seen.lock().map_err(|_| state_error())?,
        vec![json!("original\0custom-input"), Value::Null]
    );
    Ok(())
}

#[tokio::test]
async fn default_secret_failure_uses_same_manager_and_null_bypasses_it() -> Result<()> {
    let model = concrete_client(json!({"secret":"previous-storage"}))?;
    let (probe, calls) = driver_probe()?;
    assert!(
        matches!(model.set_secret(&probe,Some("default-input")).await,
        Err(error) if error.code()=="client_fixture_hash_driver_entered")
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(model.plain_secret()?.as_deref(), Some("default-input"));
    assert_eq!(model.secret()?.as_deref(), Some("previous-storage"));
    model.set_secret(&probe, None).await?;
    assert_eq!(model.secret()?, None);
    assert_eq!(model.plain_secret()?, None);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(!model.confidential()?);
    Ok(())
}

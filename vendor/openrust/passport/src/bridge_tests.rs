//! 检出 Bridge 投影绕过自定义模型、主键降精度、名称默认和 grant 比较偏差。

use super::BridgeClient;
use crate::{Client, ClientFactory, ClientModel, ClientModelFactory};
use openrust::{
    Error, Result,
    database::eloquent::{ModelIdentity, ModelMetadata},
    hashing::HashManager,
    support::BoxFuture,
};
use serde_json::{Map, Value, json};
use std::sync::Arc;

fn attributes(value: Value) -> Result<Map<String, Value>> {
    let Value::Object(attributes) = value else {
        return Err(Error::custom(
            "test_client_attributes_invalid",
            "测试模型属性必须是对象",
        ));
    };
    Ok(attributes)
}

fn client(value: Value) -> Result<Client> {
    Ok(Client::from_attributes(
        Arc::new(ClientFactory::default().metadata().clone()),
        attributes(value)?,
    ))
}

#[test]
fn projection_copies_only_protocol_fields_and_is_independent_of_model_changes() -> Result<()> {
    let model = client(json!({
        "id":"000123", "name":"应用", "secret":"encoded-secret",
        "plain_secret":"must-not-project", "provider":"accounts",
        "redirect_uris":["https://app.test/callback", "", "https://app.test/callback"],
        "grant_types":["authorization_code", "refresh_token", "authorization_code"],
        "revoked":true
    }))?;
    let projected = BridgeClient::from_client_model(&model)?;
    assert_eq!(projected.get_identifier(), "000123");
    assert_eq!(projected.get_name()?, "应用");
    assert_eq!(
        projected.get_redirect_uri().as_value(),
        &json!(["https://app.test/callback", "", "https://app.test/callback"])
    );
    assert!(projected.is_confidential());
    assert_eq!(projected.provider.as_deref(), Some("accounts"));
    assert_eq!(
        projected.grant_types.as_value(),
        &json!(["authorization_code", "refresh_token", "authorization_code"])
    );
    let debug = format!("{projected:?}");
    assert!(!debug.contains("encoded-secret"));
    assert!(!debug.contains("must-not-project"));
    model.set_raw_attributes(attributes(json!({"id":7,"name":"已修改"}))?)?;
    assert_eq!(projected.get_identifier(), "000123");
    assert_eq!(projected.get_name()?, "应用");
    Ok(())
}

#[test]
fn protocol_key_string_conversion_preserves_integer_precision_and_original_strings() -> Result<()> {
    for (key, expected) in [
        (json!(9_007_199_254_740_993i64), "9007199254740993"),
        (json!(u64::MAX), "18446744073709551615"),
        (json!(u64::MAX - 1), "18446744073709551614"),
        (json!(i64::MAX), "9223372036854775807"),
        (json!(9_223_372_036_854_775_808u64), "9223372036854775808"),
        (json!("18446744073709551615"), "18446744073709551615"),
        (json!(i64::MIN), "-9223372036854775808"),
        (json!(0), "0"),
        (json!("000123"), "000123"),
        (
            json!("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        ),
        (json!(""), ""),
    ] {
        let model = client(json!({"id":key,"name":"应用"}))?;
        assert_eq!(
            BridgeClient::from_client_model(&model)?.get_identifier(),
            expected
        );
        assert_eq!(model.model_key()?, key);
    }
    Ok(())
}

// 两个精确 Rust 整数不能因 PHP 浮点规范化被压成同一个 OAuth 客户端 ID。
#[test]
fn adjacent_unsigned_native_keys_have_distinct_protocol_identifiers() -> Result<()> {
    let left = client(json!({"id":u64::MAX - 1,"name":"左客户端"}))?;
    let right = client(json!({"id":u64::MAX,"name":"右客户端"}))?;
    let left = BridgeClient::from_client_model(&left)?;
    let right = BridgeClient::from_client_model(&right)?;
    assert_ne!(left.get_identifier(), right.get_identifier());
    Ok(())
}

#[test]
fn floating_model_keys_keep_php_scalar_conversion_instead_of_integer_recovery() -> Result<()> {
    for (key, expected) in [
        (json!(1.0), "1"),
        (json!(-0.0), "-0"),
        (json!(1.25), "1.25"),
        (json!(1e20), "1.0E+20"),
        (json!(2.0_f64.powi(64)), "1.844674407371E+19"),
    ] {
        let model = client(json!({"id":key,"name":"浮点键"}))?;
        assert_eq!(
            BridgeClient::from_client_model(&model)?.get_identifier(),
            expected
        );
        assert_eq!(model.model_key()?, key);
    }
    Ok(())
}

#[test]
fn model_key_name_is_selected_by_model_metadata_instead_of_fixed_id() -> Result<()> {
    let mut metadata = ModelMetadata::new("custom_clients", "AppClient");
    metadata.key_name = "client_key".into();
    let model = Client::from_attributes(
        Arc::new(metadata),
        attributes(json!({"id":"wrong-key","client_key":42,"name":"应用"}))?,
    );
    assert_eq!(
        BridgeClient::from_client_model(&model)?.get_identifier(),
        "42"
    );
    Ok(())
}

#[test]
fn invalid_model_key_types_fail_without_coercing_a_protocol_identity() -> Result<()> {
    for key in [Value::Null, json!([1]), json!({"a":1})] {
        let model = client(json!({"id":key,"name":"应用"}))?;
        assert!(matches!(
            BridgeClient::from_client_model(&model),
            Err(error) if error.code() == "passport_bridge_client_identifier_invalid"
                && error.status().as_u16() == 500
        ));
    }
    Ok(())
}

#[test]
fn null_name_preserves_uninitialized_error_while_empty_name_is_valid() -> Result<()> {
    let unnamed = BridgeClient::new("id", None, vec![], false, None, vec![]);
    assert!(matches!(
        unnamed.get_name(),
        Err(error) if error.code() == "passport_bridge_client_name_uninitialized"
            && error.status().as_u16() == 500
    ));
    let empty = BridgeClient::new("id", Some(String::new()), vec![], false, None, vec![]);
    assert_eq!(empty.get_name()?, "");
    assert_eq!(unnamed.get_redirect_uri().as_value(), &json!([]));
    assert!(!unnamed.is_confidential());
    assert_eq!(unnamed.provider, None);
    assert!(!unnamed.supports_grant_type("authorization_code"));
    let model = client(json!({"id":"id"}))?;
    assert!(BridgeClient::from_client_model(&model)?.get_name().is_err());
    Ok(())
}

#[test]
fn public_projection_uses_model_redirect_and_legacy_grant_fallbacks() -> Result<()> {
    let model = client(json!({
        "id":"public-client", "name":"公开应用", "secret":null,
        "redirect":"https://app.test/a,,https://app.test/b", "password_client":true
    }))?;
    let projected = BridgeClient::from_client_model(&model)?;
    assert!(!projected.is_confidential());
    assert_eq!(projected.provider, None);
    assert_eq!(
        projected.get_redirect_uri().as_value(),
        &json!(["https://app.test/a", "", "https://app.test/b"])
    );
    assert_eq!(
        projected.grant_types.as_value(),
        &json!([
            "authorization_code",
            "implicit",
            "password",
            "refresh_token",
            "urn:ietf:params:oauth:grant-type:device_code"
        ])
    );
    Ok(())
}

#[test]
fn grant_membership_retains_php_numeric_string_equality_without_prefix_or_case_matching() {
    let mut entity = BridgeClient::new(
        "id",
        Some("应用".into()),
        vec![],
        false,
        None,
        vec![
            "authorization_code".into(),
            "000123".into(),
            "1e9999".into(),
        ],
    );
    assert!(entity.supports_grant_type("authorization_code"));
    assert!(entity.supports_grant_type("123"));
    assert!(!entity.supports_grant_type("Authorization_code"));
    assert!(!entity.supports_grant_type("authorization"));
    assert!(!entity.supports_grant_type("123tail"));
    assert!(!entity.supports_grant_type("2e9999"));
    entity.grant_types = vec!["refresh_token".into()].into();
    assert!(!entity.supports_grant_type("authorization_code"));
    assert!(entity.supports_grant_type("refresh_token"));
    entity.provider = Some("custom-provider".into());
    assert_eq!(entity.provider.as_deref(), Some("custom-provider"));
    entity.set_identifier("replaced");
    assert_eq!(entity.get_identifier(), "replaced");
    entity.set_identifier("");
    assert_eq!(entity.get_identifier(), "");
}

struct VirtualClient {
    metadata: ModelMetadata,
    failure: Option<&'static str>,
}

impl VirtualClient {
    fn field<T>(&self, name: &str, value: T) -> Result<T> {
        if self.failure == Some(name) {
            Err(Error::custom_with_status(
                openrust::http::axum::http::StatusCode::CONFLICT,
                "test_virtual_client_failed",
                name,
            ))
        } else {
            Ok(value)
        }
    }
}

impl ModelIdentity for VirtualClient {
    fn model_metadata(&self) -> &ModelMetadata {
        &self.metadata
    }
    fn model_key(&self) -> Result<Value> {
        self.field("key", json!(99))
    }
}

impl ClientModel for VirtualClient {
    fn attributes(&self) -> Result<Map<String, Value>> {
        Err(Error::custom(
            "test_raw_access_forbidden",
            "协议投影不读取原始属性",
        ))
    }
    fn set_raw_attributes(&self, _: Map<String, Value>) -> Result<()> {
        Err(Error::custom(
            "test_mutation_forbidden",
            "协议投影不修改模型",
        ))
    }
    fn plain_secret(&self) -> Result<Option<String>> {
        Err(Error::custom(
            "test_secret_access_forbidden",
            "协议投影不读取明文",
        ))
    }
    fn set_secret<'a>(
        &'a self,
        _: &'a HashManager,
        _: Option<&'a str>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async {
            Err(Error::custom(
                "test_mutation_forbidden",
                "协议投影不赋值secret",
            ))
        })
    }
    fn name(&self) -> Result<Option<String>> {
        self.field("name", Some("虚拟客户端".into()))
    }
    fn redirect_uris(&self) -> Result<crate::PhpArray> {
        self.field(
            "redirects",
            vec!["https://virtual.test/callback".to_owned()],
        )
        .map(Into::into)
    }
    fn confidential(&self) -> Result<bool> {
        self.field("confidential", true)
    }
    fn provider(&self) -> Result<Option<String>> {
        self.field("provider", Some("virtual-users".into()))
    }
    fn grant_types(&self) -> Result<crate::PhpArray> {
        self.field("grants", vec!["authorization_code".to_owned()])
            .map(Into::into)
    }
    fn revoked(&self) -> Result<bool> {
        Err(Error::custom(
            "test_active_filter_forbidden",
            "活动筛选属于仓储",
        ))
    }
}

#[test]
fn custom_client_model_accessors_are_the_projection_source_without_secret_or_active_lookup()
-> Result<()> {
    let model = VirtualClient {
        metadata: ModelMetadata::new("virtual_clients", "VirtualClient"),
        failure: None,
    };
    let entity = BridgeClient::from_client_model(&model)?;
    assert_eq!(entity.get_identifier(), "99");
    assert_eq!(entity.get_name()?, "虚拟客户端");
    assert_eq!(
        entity.get_redirect_uri().as_value(),
        &json!(["https://virtual.test/callback"])
    );
    assert!(entity.is_confidential());
    assert_eq!(entity.provider.as_deref(), Some("virtual-users"));
    assert_eq!(
        entity.grant_types.as_value(),
        &json!(["authorization_code"])
    );
    Ok(())
}

#[test]
fn projection_propagates_every_model_accessor_failure() {
    for field in [
        "key",
        "name",
        "redirects",
        "confidential",
        "provider",
        "grants",
    ] {
        let model = VirtualClient {
            metadata: ModelMetadata::new("virtual_clients", "VirtualClient"),
            failure: Some(field),
        };
        assert!(matches!(
            BridgeClient::from_client_model(&model),
            Err(error) if error.code() == "test_virtual_client_failed"
                && error.status().as_u16() == 409
        ));
    }
}

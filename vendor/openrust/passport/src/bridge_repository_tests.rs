//! Bridge 校验使用真实 Client/`HashManager`；替换仓储只控制持久读取结果。

use super::*;
use crate::{ClientFactory, ClientModel, ClientModelFactory, ClientRecord};
use openrust::container::{Container, ContainerBuilder};
use openrust::{
    Error,
    database::eloquent::{AttributeMap, AttributeValue},
    hashing::{ArgonConfig, BcryptConfig, HashingConfig},
    support::BoxFuture,
};
use serde_json::{Map, json};
use std::sync::atomic::{AtomicUsize, Ordering};

struct SelectedClient {
    record: Option<ClientRecord>,
    fail: bool,
    calls: AtomicUsize,
}
impl ClientLookup for SelectedClient {
    fn find<'a>(
        &'a self,
        _scope: &'a crate::ClientLookupContext<'a>,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<ClientRecord>>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(identifier, &json!("client"));
            if self.fail {
                return Err(Error::custom_with_status(
                    openrust::http::axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "test_client_lookup_failed",
                    "受控持久读取失败",
                ));
            }
            Ok(self.record.clone())
        })
    }
}
fn container() -> Container {
    let mut builder = ContainerBuilder::new();
    builder.scoped::<crate::repository::ClientLookupCache, _>(|_| {
        Box::pin(async { Ok(Arc::new(crate::repository::ClientLookupCache::default())) })
    });
    builder.build()
}
fn hashing() -> Result<Arc<HashManager>> {
    Ok(Arc::new(HashManager::new(HashingConfig {
        bcrypt: BcryptConfig {
            rounds: 4,
            ..Default::default()
        },
        ..Default::default()
    })?))
}
fn lookup(secret: Option<&str>, revoked: bool) -> Result<Arc<SelectedClient>> {
    let attributes: Map<String, Value> = Map::from_iter([
        ("id".into(), json!("client")),
        ("name".into(), json!("客户端")),
        ("provider".into(), json!("accounts")),
        (
            "redirect_uris".into(),
            json!("[\"https://app.test/callback\"]"),
        ),
        ("grant_types".into(), json!("[\"authorization_code\"]")),
        ("secret".into(), json!(secret)),
        ("revoked".into(), json!(revoked)),
    ]);
    Ok(Arc::new(SelectedClient {
        record: Some(ClientFactory::default().new_from_builder(attributes)?),
        fail: false,
        calls: AtomicUsize::new(0),
    }))
}

struct SecretGetter(Value);
impl openrust::database::eloquent::AttributeGetter for SecretGetter {
    fn get(
        &self,
        _raw: Option<&AttributeValue>,
        _attributes: &AttributeMap,
    ) -> Result<Option<AttributeValue>> {
        Ok(Some(self.0.clone().into()))
    }
}

struct FailedSecretGetter;
impl openrust::database::eloquent::AttributeGetter for FailedSecretGetter {
    fn get(
        &self,
        _raw: Option<&AttributeValue>,
        _attributes: &AttributeMap,
    ) -> Result<Option<AttributeValue>> {
        Err(Error::custom_with_status(
            openrust::http::axum::http::StatusCode::CONFLICT,
            "test_secret_getter_failed",
            "受控 secret 读取失败",
        ))
    }
}

#[tokio::test]
async fn secret_scalar_conversion_uses_core_cast_and_getter_errors_propagate() -> Result<()> {
    let model = Arc::new(crate::Client::from_attributes(
        Arc::new(ClientFactory::default().metadata().clone()),
        Map::from_iter([
            ("id".into(), json!("client")),
            ("secret".into(), json!("raw")),
        ]),
    ));
    let clients = Arc::new(SelectedClient {
        record: Some(model.clone()),
        fail: false,
        calls: AtomicUsize::new(0),
    });
    let hashing = Arc::new(HashManager::new(HashingConfig {
        driver: "failing".into(),
        ..Default::default()
    })?);
    hashing.extend("failing", || {
        Err(Error::custom_with_status(
            openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "test_hash_driver_failed",
            "受控哈希驱动失败",
        ))
    })?;
    let bridge = BridgeClientRepository::new(clients, hashing);
    let container = container();
    let scope = container.scope()?;
    for scalar in [json!(true), json!(7), json!(1.5), json!(u64::MAX)] {
        model
            .model_attributes()
            .register_getter("secret", Arc::new(SecretGetter(scalar)))?;
        assert!(
            matches!(bridge.validate_client(&scope, "client", Some(b"input"), None).await,
            Err(error) if error.code() == "test_hash_driver_failed")
        );
    }
    for composite in [json!([]), json!({})] {
        model
            .model_attributes()
            .register_getter("secret", Arc::new(SecretGetter(composite)))?;
        assert!(
            matches!(bridge.validate_client(&scope, "client", Some(b"input"), None).await,
            Err(error) if error.code() == "model_attribute_cast_failed"
                && error.status() == openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        );
    }
    model
        .model_attributes()
        .register_getter("secret", Arc::new(FailedSecretGetter))?;
    assert!(
        matches!(bridge.validate_client(&scope, "client", Some(b"input"), None).await,
        Err(error) if error.code() == "test_secret_getter_failed"
            && error.status() == openrust::http::axum::http::StatusCode::CONFLICT)
    );
    assert_eq!(model.secret()?.as_deref(), Some("raw"));
    scope.close();
    Ok(())
}

#[tokio::test]
async fn secret_validation_uses_registered_getter_while_confidential_keeps_raw_secret() -> Result<()>
{
    let hashing = hashing()?;
    let encoded = hashing.make("correct", &HashOptions::default()).await?;
    let baseline = lookup(Some("raw-not-a-hash"), false)?;
    let attributes = baseline
        .record
        .as_ref()
        .ok_or_else(|| Error::custom("test_client_missing", "缺失客户端"))?
        .attributes()?;
    let model = Arc::new(crate::Client::from_attributes(
        Arc::new(ClientFactory::default().metadata().clone()),
        attributes,
    ));
    model
        .model_attributes()
        .register_getter("secret", Arc::new(SecretGetter(json!(encoded))))?;
    let clients = Arc::new(SelectedClient {
        record: Some(model.clone()),
        fail: false,
        calls: AtomicUsize::new(0),
    });
    let bridge = BridgeClientRepository::new(clients, hashing);
    let container = container();
    let scope = container.scope()?;
    assert!(
        bridge
            .validate_client(&scope, "client", Some(b"correct"), None)
            .await?
    );
    model
        .model_attributes()
        .register_getter("secret", Arc::new(SecretGetter(Value::Null)))?;
    assert!(
        !bridge
            .validate_client(&scope, "client", Some(b"correct"), None)
            .await?
    );
    assert!(
        bridge
            .get_client_entity(&scope, "client")
            .await?
            .ok_or_else(|| Error::custom("test_entity_missing", "未投影客户端"))?
            .is_confidential()
    );
    assert_eq!(model.secret()?.as_deref(), Some("raw-not-a-hash"));
    scope.close();
    Ok(())
}

#[tokio::test]
async fn entity_projection_uses_selected_active_model_without_secret_validation() -> Result<()> {
    let clients = lookup(Some("not-a-hash"), false)?;
    let bridge = BridgeClientRepository::new(clients.clone(), hashing()?);
    let container = container();
    let scope = container.scope()?;
    let entity = bridge
        .get_client_entity(&scope, "client")
        .await?
        .ok_or_else(|| Error::custom("test_entity_missing", "未投影客户端"))?;
    assert_eq!(entity.get_identifier(), "client");
    assert_eq!(entity.get_name()?, "客户端");
    assert_eq!(
        entity.get_redirect_uri().as_value(),
        &json!(["https://app.test/callback"])
    );
    assert!(entity.is_confidential());
    assert_eq!(entity.provider.as_deref(), Some("accounts"));
    assert!(entity.supports_grant_type("authorization_code"));
    assert!(!entity.supports_grant_type("password"));
    assert_eq!(clients.calls.load(Ordering::SeqCst), 1);
    scope.close();
    Ok(())
}

#[tokio::test]
async fn actual_default_hasher_checks_secret_and_grant_parameter_does_not_filter() -> Result<()> {
    let hashing = hashing()?;
    let encoded = hashing.make("correct", &HashOptions::default()).await?;
    let clients = lookup(Some(&encoded), false)?;
    let bridge = BridgeClientRepository::new(clients.clone(), hashing);
    let container = container();
    let scope = container.scope()?;
    for grant in [None, Some("authorization_code"), Some("not-supported")] {
        assert!(
            bridge
                .validate_client(&scope, "client", Some(b"correct"), grant)
                .await?
        );
    }
    for secret in [
        None,
        Some(b"".as_slice()),
        Some(b"0".as_slice()),
        Some(b"wrong".as_slice()),
    ] {
        assert!(
            !bridge
                .validate_client(&scope, "client", secret, None)
                .await?
        );
    }
    assert_eq!(clients.calls.load(Ordering::SeqCst), 7);
    let record = clients
        .record
        .as_ref()
        .ok_or_else(|| Error::custom("test_client_missing", "缺失客户端"))?;
    assert_eq!(record.secret()?.as_deref(), Some(encoded.as_str()));
    assert!(record.plain_secret()?.is_none());
    scope.close();
    Ok(())
}

#[tokio::test]
async fn public_revoked_and_missing_clients_fail_without_credential_success() -> Result<()> {
    let container = container();
    let scope = container.scope()?;
    for clients in [
        lookup(None, false)?,
        lookup(Some("ignored"), true)?,
        Arc::new(SelectedClient {
            record: None,
            fail: false,
            calls: AtomicUsize::new(0),
        }),
    ] {
        let bridge = BridgeClientRepository::new(clients, hashing()?);
        assert!(
            !bridge
                .validate_client(&scope, "client", Some(b"input"), None)
                .await?
        );
    }
    let public = BridgeClientRepository::new(lookup(None, false)?, hashing()?);
    assert!(
        !public
            .get_client_entity(&scope, "client")
            .await?
            .ok_or_else(|| Error::custom("test_entity_missing", "未投影公开客户端"))?
            .is_confidential()
    );
    for clients in [
        lookup(None, true)?,
        Arc::new(SelectedClient {
            record: None,
            fail: false,
            calls: AtomicUsize::new(0),
        }),
    ] {
        assert!(
            BridgeClientRepository::new(clients, hashing()?)
                .get_client_entity(&scope, "client")
                .await?
                .is_none()
        );
    }
    scope.close();
    Ok(())
}

#[tokio::test]
async fn whitespace_secret_is_not_php_empty_and_driver_errors_are_not_invalid_client() -> Result<()>
{
    let hashing = hashing()?;
    let encoded = hashing.make(" ", &HashOptions::default()).await?;
    let bridge = BridgeClientRepository::new(lookup(Some(&encoded), false)?, hashing);
    let container = container();
    let scope = container.scope()?;
    assert!(
        bridge
            .validate_client(&scope, "client", Some(b" "), None)
            .await?
    );
    let hashing = Arc::new(HashManager::new(HashingConfig {
        driver: "failing".into(),
        ..Default::default()
    })?);
    hashing.extend("failing", || {
        Err(Error::custom_with_status(
            openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "test_hash_driver_failed",
            "受控哈希驱动失败",
        ))
    })?;
    let bridge = BridgeClientRepository::new(lookup(Some("encoded"), false)?, hashing);
    let result = bridge
        .validate_client(&scope, "client", Some(b"input"), None)
        .await;
    assert!(matches!(result, Err(error) if error.code() == "test_hash_driver_failed"));
    // empty 检查先于 HashManager，不触发失败驱动。
    assert!(
        !bridge
            .validate_client(&scope, "client", Some(b"0"), None)
            .await?
    );
    scope.close();
    Ok(())
}

#[tokio::test]
async fn lookup_errors_propagate_even_when_supplied_secret_is_empty() -> Result<()> {
    let clients = Arc::new(SelectedClient {
        record: None,
        fail: true,
        calls: AtomicUsize::new(0),
    });
    let bridge = BridgeClientRepository::new(clients.clone(), hashing()?);
    let container = container();
    let scope = container.scope()?;
    assert!(
        matches!(bridge.get_client_entity(&scope, "client").await, Err(error) if error.code() == "test_client_lookup_failed")
    );
    assert!(
        matches!(bridge.validate_client(&scope, "client", None, None).await, Err(error) if error.code() == "test_client_lookup_failed")
    );
    assert_eq!(clients.calls.load(Ordering::SeqCst), 2);
    scope.close();
    Ok(())
}

// 公开合成PHP实际hash，来自actual SHA876a92852746bacfe9dd18225337e2fec188a884abcafa726d180a330427348d。
// profile分别为bcrypt-verify-true-limit-none、argon2i-verify-true、argon2id-verify-true；
// 明确只hydrate已编码secret，模拟合法既有DB行，不调用尚未支持binary输入的Client setter。
const PUBLIC_PHP_BINARY_HASHES: [(&str, &str, &[u8], &str); 6] = [
    (
        "bcrypt",
        "invalid-utf8-single",
        b"\xff",
        "$2y$04$CssGPkYHS3O/PfIMl4dQUuURaBqEtV6cXZ0OsB97lHKjffBqgCS0O",
    ),
    (
        "bcrypt",
        "invalid-utf8-truncated",
        b"\xe4\xb8",
        "$2y$04$5jRaeeEuxjNcjpz5o0OxK.FbfsTlgs2tnVJ7ABi2Y3eYRf26og4NG",
    ),
    (
        "argon2i",
        "invalid-utf8-single",
        b"\xff",
        "$argon2i$v=19$m=1024,t=1,p=1$V2RxbEkwcVdQZGQyWlFObA$eEYoPgiuX2kGQdGc6r5JucjSz6cI13AVrO+nLbSr48U",
    ),
    (
        "argon2i",
        "invalid-utf8-truncated",
        b"\xe4\xb8",
        "$argon2i$v=19$m=1024,t=1,p=1$Ym5GNmlNZVY0NzhHLjlxMw$MB8mUktO8coNS4U7zAvz/2rRSgZbUgv9XgpcD4RRS5Q",
    ),
    (
        "argon2id",
        "invalid-utf8-single",
        b"\xff",
        "$argon2id$v=19$m=1024,t=1,p=1$NjB6eGVtblhOSGV1THVBVQ$xSRTAQaLQPQdEvkp0fEaXkbDJdd3oYgtijzNfxkrP24",
    ),
    (
        "argon2id",
        "invalid-utf8-truncated",
        b"\xe4\xb8",
        "$argon2id$v=19$m=1024,t=1,p=1$TkVwMHZoU05jR1N2TFBvWg$+S1J95bvX0juv7OCW6nIs5k16NDEmy7XLuieAjxJnQU",
    ),
];

fn binary_fixture_hashing(driver: &str) -> Result<Arc<HashManager>> {
    Ok(Arc::new(HashManager::new(HashingConfig {
        driver: driver.to_owned(),
        bcrypt: BcryptConfig {
            rounds: 4,
            verify: true,
            limit: None,
        },
        argon: ArgonConfig {
            memory: 1024,
            time: 1,
            threads: 1,
            verify: true,
        },
        ..Default::default()
    })?))
}

#[tokio::test]
async fn real_php_binary_hashes_validate_through_selected_client_and_same_manager() -> Result<()> {
    let container = container();
    let scope = container.scope()?;
    for (algorithm, _case, secret, encoded) in PUBLIC_PHP_BINARY_HASHES {
        let clients = lookup(Some(encoded), false)?;
        // PHP PHC 使用 argon2i，应用 HashManager 的同一驱动登记为 argon。
        let driver = if algorithm == "argon2i" {
            "argon"
        } else {
            algorithm
        };
        let bridge = BridgeClientRepository::new(clients.clone(), binary_fixture_hashing(driver)?);
        assert!(
            bridge
                .validate_client(&scope, "client", Some(secret), None)
                .await?
        );
        // 替换成Unicode replacement bytes必须失败，不能以lossy UTF8偶然通过。
        assert!(
            !bridge
                .validate_client(&scope, "client", Some(b"\xef\xbf\xbd"), None)
                .await?
        );
        assert_eq!(clients.calls.load(Ordering::SeqCst), 2);
        let record = clients
            .record
            .as_ref()
            .ok_or_else(|| Error::custom("test_client_missing", "缺失客户端"))?;
        assert_eq!(record.secret()?.as_deref(), Some(encoded));
        assert!(record.plain_secret()?.is_none());
    }
    scope.close();
    Ok(())
}

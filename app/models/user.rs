//! 应用账户属性适配；复用同一UserProvider，不创建第二份账户存储。

use openrust::{
    Result,
    auth::{Authenticatable, GenericUser},
};
use serde_json::{Map, Value};

#[cfg(feature = "auth-fortify")]
use openrust::auth::fortify::DatabaseFactorAuthority;
#[cfg(feature = "auth-fortify")]
use openrust::auth::fortify::DatabaseTwoFactorState;
#[cfg(feature = "auth-fortify")]
use openrust::auth::fortify::TwoFactorAuthenticatable;
#[cfg(feature = "auth-fortify")]
use openrust::auth::fortify::TwoFactorState;
#[cfg(feature = "auth-fortify")]
use openrust::support::BoxFuture;

/// 当前应用账户模型；密码列沿用已有存储字段，其他应用可实现自己的 `Authenticatable`。
pub struct User {
    metadata: std::sync::Arc<openrust::database::eloquent::ModelMetadata>,
    attributes: GenericUser,
    #[cfg(feature = "auth-fortify")]
    two_factor_state: Option<DatabaseTwoFactorState>,
    #[cfg(feature = "auth-fortify")]
    factor_authority: Option<DatabaseFactorAuthority>,
}
impl User {
    /// 同一账户模型的原始属性与访问器管线，供应用登记公开属性扩展。
    pub fn model_attributes(&self) -> &openrust::database::eloquent::ModelAttributes {
        self.attributes.model_attributes()
    }
    /// 选择同一应用模型的实际表、连接和主键；默认认证标识与模型主键一致。
    #[must_use]
    pub fn with_model_metadata(
        mut self,
        metadata: std::sync::Arc<openrust::database::eloquent::ModelMetadata>,
    ) -> Self {
        self.metadata = metadata;
        self
    }

    /// 从数据库Provider已转换的属性构造账户，不要求邮箱或手机号存在。
    pub fn new(attributes: Map<String, Value>) -> Self {
        Self {
            metadata: std::sync::Arc::new(openrust::database::eloquent::ModelMetadata::new(
                "users",
                "app::models::user::User",
            )),
            attributes: GenericUser::new(attributes),
            #[cfg(feature = "auth-fortify")]
            two_factor_state: None,
            #[cfg(feature = "auth-fortify")]
            factor_authority: None,
        }
    }

    /// 显式接入同一模型持久来源的双因素状态；仅启用feature不会自动开启此能力。
    #[cfg(feature = "auth-fortify")]
    #[must_use]
    pub fn with_two_factor_state(mut self, state: DatabaseTwoFactorState) -> Self {
        self.two_factor_state = Some(state);
        self
    }

    /// 选择与本模型同源的旧/新因子状态、接管及持久代际保护。
    #[cfg(feature = "auth-fortify")]
    #[must_use]
    pub fn with_factor_authority(mut self, authority: DatabaseFactorAuthority) -> Self {
        self.factor_authority = Some(authority);
        self
    }

    #[cfg(feature = "auth-fortify")]
    fn two_factor_store(&self) -> Result<&DatabaseTwoFactorState> {
        self.two_factor_state.as_ref().ok_or_else(|| {
            openrust::Error::custom_with_status(
                openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "app_two_factor_state_unavailable",
                "应用账户未接入双因素持久状态",
            )
        })
    }
}
impl openrust::database::eloquent::ModelIdentity for User {
    fn model_metadata(&self) -> &openrust::database::eloquent::ModelMetadata {
        &self.metadata
    }
    fn model_key(&self) -> Result<Value> {
        Ok(self
            .attributes
            .model_attributes()
            .get_attribute(&self.metadata, &self.metadata.key_name)?
            .unwrap_or(Value::Null))
    }
}
impl Authenticatable for User {
    fn as_model(&self) -> Option<&dyn openrust::database::eloquent::ModelIdentity> {
        Some(self)
    }
    fn auth_attribute(&self, name: &str) -> Result<Option<Value>> {
        self.model_attributes().get_attribute(&self.metadata, name)
    }

    #[cfg(feature = "auth-fortify")]
    fn as_two_factor(&self) -> Option<&dyn TwoFactorAuthenticatable> {
        self.two_factor_state
            .as_ref()
            .map(|_| self as &dyn TwoFactorAuthenticatable)
    }
    fn auth_identifier_name(&self) -> &str {
        &self.metadata.key_name
    }
    fn auth_identifier(&self) -> Result<Value> {
        Ok(self
            .auth_attribute(self.auth_identifier_name())?
            .unwrap_or(Value::Null))
    }
    fn auth_password_name(&self) -> &'static str {
        "password_hash"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        match self.auth_attribute(self.auth_password_name())? {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value)),
            _ => Err(openrust::Error::custom(
                "app_password_hash_invalid",
                "应用账户密码哈希类型无效",
            )),
        }
    }
    fn remember_token(&self) -> Result<Option<String>> {
        let name = self.remember_token_name();
        if name.is_empty() {
            return Ok(None);
        }
        self.attributes
            .model_attributes()
            .get_string_attribute(&self.metadata, name)
            .map(|value| Some(value.unwrap_or_default()))
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        let name = self.remember_token_name();
        if name.is_empty() {
            return Ok(None);
        }
        let value = self
            .attributes
            .model_attributes()
            .committed_attribute(&self.metadata, name)?;
        let value = openrust::database::eloquent::AttributeCast::String.read(value)?;
        Ok(Some(
            value
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
        ))
    }
    fn set_remember_token(
        &self,
        token: Option<String>,
    ) -> openrust::support::BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let name = self.remember_token_name();
            if name.is_empty() {
                return Ok(());
            }
            self.attributes
                .model_attributes()
                .set_attribute(
                    &self.metadata,
                    name,
                    token.map_or(Value::Null, Value::String),
                )
                .await
        })
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> openrust::support::BoxFuture<'a, Result<Option<openrust::auth::RememberTokenUpdate>>> {
        Box::pin(async move {
            let name = self.remember_token_name();
            if name.is_empty() {
                return Ok(None);
            }
            self.attributes
                .model_attributes()
                .set_attribute_reversibly_if(
                    &self.metadata,
                    name,
                    replacement.map_or(Value::Null, Value::String),
                    move |value| {
                        let current = openrust::database::eloquent::AttributeCast::String
                            .read(value)?
                            .and_then(|value| value.as_str().map(str::to_owned))
                            .unwrap_or_default();
                        Ok(expected == Some(current.as_str()))
                    },
                )
                .await
                .map(|receipt| receipt.map(openrust::auth::RememberTokenUpdate::from))
        })
    }
    fn remember_token_name(&self) -> &'static str {
        "remember_token"
    }
}

#[cfg(feature = "auth-fortify")]
impl TwoFactorAuthenticatable for User {
    fn two_factor_authority(&self) -> Option<&DatabaseFactorAuthority> {
        self.factor_authority.as_ref()
    }
    fn two_factor_state(&self) -> BoxFuture<'_, Result<TwoFactorState>> {
        Box::pin(async move { self.two_factor_store()?.load().await })
    }

    fn compare_and_set_two_factor_state<'a>(
        &'a self,
        expected: &'a TwoFactorState,
        replacement: &'a TwoFactorState,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            if self.factor_authority.is_some() {
                return Err(openrust::Error::custom(
                    "app_factor_authority_required",
                    "已选择统一因子策略，请通过公开Actions更新状态及代际",
                ));
            }
            self.two_factor_store()?
                .compare_and_set(expected, replacement)
                .await
        })
    }

    fn two_factor_account_name(&self, username: &str) -> Result<Option<String>> {
        if username == "account" {
            // account是组合登录的虚拟输入名，不能作为持久属性或必填联系方式。
            for field in ["username", "email", "phone"] {
                if let Some(label) = self.two_factor_account_name(field)?
                    && !label.is_empty()
                {
                    return Ok(Some(label));
                }
            }
            return match self.auth_identifier()? {
                Value::String(value) => Ok(Some(value)),
                Value::Number(value) => Ok(Some(value.to_string())),
                _ => Err(openrust::Error::custom(
                    "app_two_factor_identifier_label_invalid",
                    "账户标识不能用作双因素标签",
                )),
            };
        }
        match self.attributes.attribute(username)? {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value)),
            _ => Err(openrust::Error::custom_with_status(
                openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "app_two_factor_account_name_invalid",
                "应用账户的双因素标签字段类型无效",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_model_key_cast_preserves_raw_attributes_and_allows_accessor_override()
    -> openrust::Result<()> {
        use openrust::{
            auth::Authenticatable,
            database::eloquent::{AttributeGetter, ModelIdentity},
        };
        use serde_json::{Map, Value, json};
        struct PrefixedKey;
        impl AttributeGetter for PrefixedKey {
            fn get(
                &self,
                raw: Option<&Value>,
                _: &Map<String, Value>,
            ) -> openrust::Result<Option<Value>> {
                Ok(raw
                    .and_then(Value::as_str)
                    .map(|value| json!(format!("account:{value}"))))
            }
        }
        let user = super::User::new(Map::from_iter([("id".into(), json!("00042"))]));
        assert_eq!(user.model_key()?, json!(42));
        assert_eq!(user.auth_identifier()?, json!(42));
        assert_eq!(user.auth_attribute("id")?, Some(json!(42)));
        assert_eq!(user.attributes.attribute("id")?, Some(json!("00042")));
        user.attributes
            .model_attributes()
            .register_getter("id", std::sync::Arc::new(PrefixedKey))?;
        assert_eq!(user.model_key()?, json!("account:00042"));
        assert_eq!(user.auth_identifier()?, json!("account:00042"));
        assert_eq!(user.auth_attribute("id")?, Some(json!("account:00042")));
        assert_eq!(user.attributes.attribute("id")?, Some(json!("00042")));
        Ok(())
    }
    #[test]
    fn default_auth_identifier_tracks_the_selected_model_key() -> openrust::Result<()> {
        use openrust::auth::Authenticatable;
        let mut metadata =
            openrust::database::eloquent::ModelMetadata::new("accounts", "app::Account");
        metadata.key_name = "uuid_key".into();
        metadata.key_type = "string".into();
        metadata.incrementing = false;
        let user = super::User::new(serde_json::Map::from_iter([
            ("id".into(), serde_json::json!("00042")),
            ("uuid_key".into(), serde_json::json!("tenant:42")),
        ]))
        .with_model_metadata(std::sync::Arc::new(metadata));
        assert_eq!(user.auth_identifier_name(), "uuid_key");
        assert_eq!(user.auth_identifier()?, serde_json::json!("tenant:42"));
        let model = user.as_model().ok_or_else(|| {
            openrust::Error::custom("test_model_identity_missing", "模型身份能力缺失")
        })?;
        assert_eq!(model.model_key()?, serde_json::json!("tenant:42"));
        assert_eq!(model.morph_class()?, serde_json::json!("app::Account"));
        Ok(())
    }
    use super::*;
    use serde_json::json;

    struct LoginUser(User);
    impl openrust::database::eloquent::ModelIdentity for LoginUser {
        fn model_metadata(&self) -> &openrust::database::eloquent::ModelMetadata {
            &self.0.metadata
        }
        fn model_key(&self) -> Result<Value> {
            openrust::database::eloquent::ModelIdentity::model_key(&self.0)
        }
    }
    impl Authenticatable for LoginUser {
        fn as_model(&self) -> Option<&dyn openrust::database::eloquent::ModelIdentity> {
            Some(self)
        }
        fn auth_attribute(&self, name: &str) -> Result<Option<Value>> {
            self.0.auth_attribute(name)
        }
        fn auth_identifier_name(&self) -> &'static str {
            "id"
        }
        fn auth_identifier(&self) -> Result<Value> {
            Ok(self
                .auth_attribute(self.auth_identifier_name())?
                .unwrap_or(Value::Null))
        }
        fn auth_password_name(&self) -> &str {
            self.0.auth_password_name()
        }
        fn auth_password(&self) -> Result<Option<String>> {
            self.0.auth_password()
        }
        fn remember_token(&self) -> Result<Option<String>> {
            self.0.remember_token()
        }
        fn remember_token_for_cookie(&self) -> Result<Option<String>> {
            self.0.remember_token_for_cookie()
        }
        fn set_remember_token(
            &self,
            token: Option<String>,
        ) -> openrust::support::BoxFuture<'_, Result<()>> {
            self.0.set_remember_token(token)
        }
        fn begin_remember_token_update<'a>(
            &'a self,
            expected: Option<&'a str>,
            replacement: Option<String>,
        ) -> openrust::support::BoxFuture<'a, Result<Option<openrust::auth::RememberTokenUpdate>>>
        {
            self.0.begin_remember_token_update(expected, replacement)
        }
        fn remember_token_name(&self) -> &str {
            self.0.remember_token_name()
        }
    }

    #[test]
    fn independent_login_column_uses_only_its_explicit_cast_and_getter() -> Result<()> {
        use openrust::database::eloquent::{
            AttributeCast, AttributeGetter, ModelIdentity, ModelMetadata,
        };
        struct LoginGetter;
        impl AttributeGetter for LoginGetter {
            fn get(&self, raw: Option<&Value>, _: &Map<String, Value>) -> Result<Option<Value>> {
                Ok(raw
                    .and_then(Value::as_str)
                    .map(|value| json!(format!("login:{value}"))))
            }
        }
        let mut metadata = ModelMetadata::new("accounts", "app::Account");
        metadata.key_name = "uuid_key".into();
        metadata.key_type = "string".into();
        metadata.incrementing = false;
        metadata.casts.insert("id".into(), AttributeCast::Integer);
        let values = Map::from_iter([
            ("id".into(), json!("00042")),
            (
                "uuid_key".into(),
                json!("550e8400-e29b-41d4-a716-446655440000"),
            ),
        ]);
        let user =
            LoginUser(User::new(values.clone()).with_model_metadata(std::sync::Arc::new(metadata)));
        assert_eq!(user.auth_identifier()?, json!(42));
        assert_eq!(
            user.model_key()?,
            json!("550e8400-e29b-41d4-a716-446655440000")
        );
        user.0
            .model_attributes()
            .register_getter("id", std::sync::Arc::new(LoginGetter))?;
        assert_eq!(user.auth_identifier()?, json!("login:00042"));
        assert_eq!(user.auth_attribute("id")?, Some(json!("login:00042")));
        assert_eq!(
            user.0.attributes.model_attributes().raw_attributes()?,
            values
        );
        Ok(())
    }

    #[test]
    fn nonincrementing_uuid_identifier_is_preserved() -> Result<()> {
        use openrust::database::eloquent::{ModelIdentity, ModelMetadata};
        let mut metadata = ModelMetadata::new("accounts", "app::Account");
        metadata.key_type = "string".into();
        metadata.incrementing = false;
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let user = User::new(Map::from_iter([("id".into(), json!(uuid))]))
            .with_model_metadata(std::sync::Arc::new(metadata));
        assert_eq!(user.auth_identifier()?, json!(uuid));
        assert_eq!(user.model_key()?, json!(uuid));
        assert_eq!(user.auth_attribute("id")?, Some(json!(uuid)));
        Ok(())
    }

    #[tokio::test]
    async fn password_getter_and_setter_share_model_attributes_without_mutating_reads() -> Result<()>
    {
        use openrust::database::eloquent::{
            AttributeCast, AttributeGetter, AttributeSetter, ModelMetadata,
        };
        struct PasswordGetter;
        impl AttributeGetter for PasswordGetter {
            fn get(&self, raw: Option<&Value>, _: &Map<String, Value>) -> Result<Option<Value>> {
                Ok(raw
                    .and_then(Value::as_str)
                    .and_then(|value| value.strip_prefix("stored:"))
                    .map(|value| json!(value)))
            }
        }
        struct PasswordSetter;
        impl AttributeSetter for PasswordSetter {
            fn set(
                &self,
                value: Value,
                _: Map<String, Value>,
            ) -> openrust::support::BoxFuture<'_, Result<Map<String, Value>>> {
                Box::pin(async move {
                    Ok(Map::from_iter([
                        ("password_hash".into(), value),
                        ("secret_set".into(), json!(true)),
                    ]))
                })
            }
        }
        let mut metadata = ModelMetadata::new("users", "app::User");
        metadata
            .casts
            .insert("password_hash".into(), AttributeCast::Hashed);
        let user = User::new(Map::new()).with_model_metadata(std::sync::Arc::new(metadata));
        user.attributes
            .model_attributes()
            .register_getter("password_hash", std::sync::Arc::new(PasswordGetter))?;
        user.attributes
            .model_attributes()
            .register_setter("password_hash", std::sync::Arc::new(PasswordSetter))?;
        user.attributes
            .model_attributes()
            .set_attribute(&user.metadata, "password_hash", json!("stored:opaque-hash"))
            .await?;
        let raw = user.attributes.model_attributes().raw_attributes()?;
        assert_eq!(user.auth_password_name(), "password_hash");
        assert_eq!(user.auth_password()?.as_deref(), Some("opaque-hash"));
        assert_eq!(
            user.auth_attribute("password_hash")?,
            Some(json!("opaque-hash"))
        );
        assert_eq!(user.attributes.model_attributes().raw_attributes()?, raw);
        assert_eq!(
            user.attributes.attribute("password_hash")?,
            Some(json!("stored:opaque-hash"))
        );
        assert_eq!(
            user.attributes
                .model_attributes()
                .raw_attribute("secret_set")?,
            Some(json!(true))
        );
        Ok(())
    }

    #[test]
    fn password_return_type_preserves_null_and_rejects_uncast_nonstrings() -> Result<()> {
        use openrust::database::eloquent::{AttributeCast, ModelMetadata};
        let user = User::new(Map::from_iter([("password_hash".into(), Value::Null)]));
        assert_eq!(user.auth_password()?, None);
        user.attributes
            .model_attributes()
            .set_raw_attribute("password_hash", json!(123))?;
        assert!(
            matches!(user.auth_password(), Err(error) if error.code() == "app_password_hash_invalid")
        );
        let mut metadata = ModelMetadata::new("users", "app::User");
        metadata
            .casts
            .insert("password_hash".into(), AttributeCast::String);
        let user = user.with_model_metadata(std::sync::Arc::new(metadata));
        assert_eq!(user.auth_password()?.as_deref(), Some("123"));
        assert_eq!(
            user.attributes
                .model_attributes()
                .raw_attribute("password_hash")?,
            Some(json!(123))
        );
        Ok(())
    }

    struct RememberGetter;
    impl openrust::database::eloquent::AttributeGetter for RememberGetter {
        fn get(&self, value: Option<&Value>, _: &Map<String, Value>) -> Result<Option<Value>> {
            Ok(value
                .and_then(Value::as_str)
                .and_then(|value| value.strip_prefix("stored:"))
                .map(|value| json!(value)))
        }
    }
    struct RememberSetter(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl openrust::database::eloquent::AttributeSetter for RememberSetter {
        fn set(
            &self,
            value: Value,
            _: Map<String, Value>,
        ) -> openrust::support::BoxFuture<'_, Result<Map<String, Value>>> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                let generation = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                let value = match value {
                    Value::String(value) => json!(format!("stored:{value}")),
                    Value::Null => Value::Null,
                    _ => {
                        return Err(openrust::Error::custom(
                            "test_remember_input_invalid",
                            "记住我测试输入无效",
                        ));
                    }
                };
                Ok(Map::from_iter([
                    ("remember_token".into(), value),
                    ("remember_generation".into(), json!(generation)),
                ]))
            })
        }
    }
    #[tokio::test]
    async fn remember_model_getter_distinguishes_empty_view_from_missing_raw_and_restores_receipt()
    -> Result<()> {
        let user = User::new(Map::new());
        assert_eq!(user.remember_token()?.as_deref(), Some(""));
        assert_eq!(user.attributes.attribute("remember_token")?, None);
        assert!(
            user.begin_remember_token_update(None, Some("not-installed".into()))
                .await?
                .is_none()
        );
        let receipt = user
            .begin_remember_token_update(Some(""), Some("temporary".into()))
            .await?
            .ok_or_else(|| {
                openrust::Error::custom("test_remember_receipt_missing", "缺少记住我修改收据")
            })?;
        assert_eq!(user.remember_token()?.as_deref(), Some("temporary"));
        drop(receipt);
        assert_eq!(user.attributes.attribute("remember_token")?, None);
        let receipt = user
            .begin_remember_token_update(Some(""), Some("committed".into()))
            .await?
            .ok_or_else(|| {
                openrust::Error::custom("test_remember_receipt_missing", "缺少记住我修改收据")
            })?;
        receipt.commit()?;
        assert_eq!(user.remember_token()?.as_deref(), Some("committed"));
        Ok(())
    }

    #[tokio::test]
    async fn remember_async_setter_multi_field_patch_uses_same_getter_and_atomic_rollback()
    -> Result<()> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let user = User::new(Map::new());
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        user.attributes
            .model_attributes()
            .register_getter("remember_token", std::sync::Arc::new(RememberGetter))?;
        user.attributes.model_attributes().register_setter(
            "remember_token",
            std::sync::Arc::new(RememberSetter(calls.clone())),
        )?;
        user.set_remember_token(Some("original".into())).await?;
        let before = user.attributes.model_attributes().raw_attributes()?;
        assert_eq!(user.remember_token()?.as_deref(), Some("original"));
        assert_eq!(
            user.attributes.attribute("remember_token")?,
            Some(json!("stored:original"))
        );
        assert!(
            user.begin_remember_token_update(Some("stored:original"), Some("wrong".into()))
                .await?
                .is_none()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mut receipt = user
            .begin_remember_token_update(Some("original"), Some("replacement".into()))
            .await?
            .ok_or_else(|| {
                openrust::Error::custom("test_remember_receipt_missing", "缺少记住我修改收据")
            })?;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(user.remember_token()?.as_deref(), Some("replacement"));
        assert!(
            matches!(user.remember_token_for_cookie(), Err(error) if error.code() == "model_attribute_update_pending" && error.status().as_u16() == 409)
        );
        assert_eq!(
            user.attributes.attribute("remember_generation")?,
            Some(json!(2))
        );
        assert!(receipt.rollback()?);
        drop(receipt);
        assert_eq!(user.attributes.model_attributes().raw_attributes()?, before);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let receipt = user
            .begin_remember_token_update(Some("original"), Some("drop-restored".into()))
            .await?
            .ok_or_else(|| {
                openrust::Error::custom("test_remember_receipt_missing", "缺少记住我修改收据")
            })?;
        drop(receipt);
        assert_eq!(user.attributes.model_attributes().raw_attributes()?, before);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            user.remember_token_for_cookie()?.as_deref(),
            Some("original")
        );
        let receipt = user
            .begin_remember_token_update(Some("original"), Some("committed".into()))
            .await?
            .ok_or_else(|| {
                openrust::Error::custom("test_remember_receipt_missing", "缺少记住我修改收据")
            })?;
        assert!(
            matches!(user.remember_token_for_cookie(), Err(error) if error.code() == "model_attribute_update_pending")
        );
        receipt.commit()?;
        assert_eq!(
            user.remember_token_for_cookie()?.as_deref(),
            Some("committed")
        );
        Ok(())
    }

    struct DependentRememberGetter;
    impl openrust::database::eloquent::AttributeGetter for DependentRememberGetter {
        fn get(&self, _: Option<&Value>, raw: &Map<String, Value>) -> Result<Option<Value>> {
            Ok(raw.get("other_token").cloned())
        }
    }

    #[tokio::test]
    async fn cookie_read_rejects_pending_field_used_by_remember_getter() -> Result<()> {
        let user = User::new(Map::from_iter([("other_token".into(), json!("original"))]));
        let store = user.attributes.model_attributes();
        store.register_getter(
            "remember_token",
            std::sync::Arc::new(DependentRememberGetter),
        )?;
        let mut receipt = store
            .set_attribute_reversibly_if(&user.metadata, "other_token", json!("pending"), |_| {
                Ok(true)
            })
            .await?
            .ok_or_else(|| {
                openrust::Error::custom("test_remember_receipt_missing", "缺少记住我修改收据")
            })?;
        assert_eq!(user.remember_token()?.as_deref(), Some("pending"));
        assert!(
            matches!(user.remember_token_for_cookie(), Err(error) if error.code() == "model_attribute_update_pending")
        );
        assert!(receipt.rollback()?);
        assert_eq!(
            user.remember_token_for_cookie()?.as_deref(),
            Some("original")
        );
        Ok(())
    }

    #[test]
    fn remember_getter_uses_php_scalar_view_without_changing_raw_type() -> Result<()> {
        for (raw, expected) in [
            (json!(123), "123"),
            (json!(false), ""),
            (json!("0"), "0"),
            (Value::Null, ""),
        ] {
            let user = User::new(Map::from_iter([("remember_token".into(), raw.clone())]));
            assert_eq!(user.remember_token()?.as_deref(), Some(expected));
            assert_eq!(user.attributes.attribute("remember_token")?, Some(raw));
        }
        Ok(())
    }

    struct PausedRememberSetter {
        entered: std::sync::Arc<tokio::sync::Notify>,
        release: std::sync::Arc<tokio::sync::Notify>,
    }
    impl openrust::database::eloquent::AttributeSetter for PausedRememberSetter {
        fn set(
            &self,
            value: Value,
            _: Map<String, Value>,
        ) -> openrust::support::BoxFuture<'_, Result<Map<String, Value>>> {
            Box::pin(async move {
                self.entered.notify_one();
                self.release.notified().await;
                Ok(Map::from_iter([
                    ("remember_token".into(), value),
                    ("cancelled_side_effect".into(), json!(true)),
                ]))
            })
        }
    }
    #[tokio::test]
    async fn cancelling_user_remember_preparation_leaves_no_multi_field_patch_or_pending_owner()
    -> Result<()> {
        let user = User::new(Map::new());
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        user.attributes.model_attributes().register_setter(
            "remember_token",
            std::sync::Arc::new(PausedRememberSetter {
                entered: entered.clone(),
                release: std::sync::Arc::new(tokio::sync::Notify::new()),
            }),
        )?;
        let before = user.attributes.model_attributes().raw_attributes()?;
        let mut pending = user.begin_remember_token_update(Some(""), Some("cancelled".into()));
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            tokio::select! {
                () = entered.notified() => Ok(()),
                result = &mut pending => {
                    drop(result);
                    Err(openrust::Error::custom("test_remember_completed_early", "异步setter未进入受控等待"))
                }
            }
        }).await.map_err(openrust::Error::internal)??;
        drop(pending);
        assert_eq!(user.attributes.model_attributes().raw_attributes()?, before);
        user.attributes.model_attributes().register_setter(
            "remember_token",
            std::sync::Arc::new(RememberSetter(std::sync::Arc::new(
                std::sync::atomic::AtomicUsize::new(0),
            ))),
        )?;
        user.set_remember_token(Some("after-cancel".into())).await?;
        assert_eq!(
            user.attributes.attribute("remember_token")?,
            Some(json!("stored:after-cancel"))
        );
        Ok(())
    }

    #[tokio::test]
    async fn remember_hashed_assignment_requires_explicit_context_without_global_resolution()
    -> Result<()> {
        let mut metadata = openrust::database::eloquent::ModelMetadata::new("users", "app::User");
        metadata.casts.insert(
            "remember_token".into(),
            openrust::database::eloquent::AttributeCast::Hashed,
        );
        let user = User::new(Map::new()).with_model_metadata(std::sync::Arc::new(metadata));
        let error = user
            .set_remember_token(Some("unhashable-without-context".into()))
            .await
            .err()
            .ok_or_else(|| {
                openrust::Error::custom("test_remember_expected_error", "缺少哈希上下文时未拒绝")
            })?;
        assert_eq!(error.code(), "model_attribute_hashing_required");
        assert_eq!(
            error.status(),
            openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        assert!(
            matches!(user.begin_remember_token_update(Some(""), Some("new".into())).await,
            Err(error) if error.code() == "model_attribute_hashing_required")
        );
        assert_eq!(user.attributes.attribute("remember_token")?, None);
        user.set_remember_token(None).await?;
        assert_eq!(user.remember_token()?.as_deref(), Some(""));
        assert_eq!(
            user.attributes.attribute("remember_token")?,
            Some(Value::Null)
        );
        Ok(())
    }

    #[test]
    #[cfg(feature = "auth-fortify")]
    fn feature_alone_does_not_advertise_unbound_two_factor_capability() -> Result<()> {
        let user = User::new(Map::from_iter([
            ("id".into(), json!("550e8400-e29b-41d4-a716-446655440000")),
            ("username".into(), json!("username-only")),
        ]));
        assert!(user.as_two_factor().is_none());
        assert!(user.two_factor_store().is_err());
        assert_eq!(
            user.two_factor_account_name("username")?.as_deref(),
            Some("username-only")
        );
        assert_eq!(user.two_factor_account_name("email")?, None);
        assert_eq!(user.two_factor_account_name("phone")?, None);
        Ok(())
    }

    #[test]
    #[cfg(feature = "auth-fortify")]
    fn configured_account_label_keeps_null_distinct_from_an_invalid_model_field() -> Result<()> {
        let user = User::new(Map::from_iter([
            ("id".into(), json!(42)),
            ("display_name".into(), json!("账户标签")),
            ("email".into(), Value::Null),
            ("invalid_label".into(), json!(false)),
        ]));
        assert_eq!(user.auth_identifier()?, json!(42));
        assert_eq!(
            user.two_factor_account_name("display_name")?.as_deref(),
            Some("账户标签")
        );
        assert_eq!(user.two_factor_account_name("email")?, None);
        let error = user
            .two_factor_account_name("invalid_label")
            .err()
            .ok_or_else(|| {
                openrust::Error::custom("test_expected_model_error", "错误标签类型未被拒绝")
            })?;
        assert_eq!(error.code(), "app_two_factor_account_name_invalid");
        assert_eq!(
            error.status(),
            openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        Ok(())
    }
    #[test]
    #[cfg(feature = "auth-fortify")]
    fn virtual_account_label_does_not_require_email_phone_or_a_stored_account_field() -> Result<()>
    {
        for (attributes, expected) in [
            (
                json!({"id":"uuid-only","username":"member","email":null,"phone":null}),
                "member",
            ),
            (
                json!({"id":42,"email":"bound@example.test"}),
                "bound@example.test",
            ),
            (json!({"id":42,"phone":"+8613800000000"}), "+8613800000000"),
            (json!({"id":"uuid-only"}), "uuid-only"),
            (json!({"id":42}), "42"),
        ] {
            let mut metadata =
                openrust::database::eloquent::ModelMetadata::new("users", "app::User");
            if attributes.get("id").is_some_and(Value::is_string) {
                metadata.key_type = "string".into();
                metadata.incrementing = false;
            }
            let user = User::new(attributes.as_object().cloned().unwrap_or_default())
                .with_model_metadata(std::sync::Arc::new(metadata));
            assert_eq!(
                user.two_factor_account_name("account")?.as_deref(),
                Some(expected)
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod attribute_snapshot_tests {
    use super::User;
    use openrust::{
        Result,
        auth::Authenticatable,
        database::eloquent::{AttributeCast, ModelIdentity, ModelMetadata},
    };
    use serde_json::{Map, json};
    use std::sync::Arc;

    #[test]
    fn public_user_attributes_keep_hydrated_original_and_explicit_changes() -> Result<()> {
        let mut metadata = ModelMetadata::new("users", "app::models::user::User");
        metadata
            .casts
            .insert("password_hash".into(), AttributeCast::Hashed);
        let user = User::new(Map::from_iter([
            ("id".into(), json!("00042")),
            ("password_hash".into(), json!("hydrated-hash")),
        ]))
        .with_model_metadata(Arc::new(metadata));
        let attributes = user.model_attributes();
        assert!(attributes.is_clean(user.model_metadata(), &[])?);
        assert_eq!(attributes.get_raw_original("id")?, Some(json!("00042")));
        attributes.set_raw_attribute("id", json!(42))?;
        assert!(attributes.is_clean(user.model_metadata(), &["id"])?);
        assert_eq!(user.auth_identifier()?, json!(42));
        attributes.set_raw_attribute("password_hash", json!("replacement-hash"))?;
        attributes.sync_changes(user.model_metadata())?;
        assert_eq!(
            attributes.get_previous()?.get("password_hash"),
            Some(&json!("hydrated-hash"))
        );
        assert_eq!(
            attributes.get_changes()?.get("password_hash"),
            Some(&json!("replacement-hash"))
        );
        assert_eq!(user.auth_password()?, Some("replacement-hash".into()));
        assert_eq!(
            attributes.get_raw_original("password_hash")?,
            Some(json!("hydrated-hash"))
        );
        Ok(())
    }
}

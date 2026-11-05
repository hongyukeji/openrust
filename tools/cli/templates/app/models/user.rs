//! 应用账户属性适配；复用同一UserProvider，不创建第二份账户存储。

use openrust::{
    Result,
    auth::{Authenticatable, GenericUser},
};
use serde_json::{Map, Value};

#[cfg(feature = "auth-fortify")]
use openrust::{
    auth::fortify::{
        DatabaseFactorAuthority, DatabaseTwoFactorState, TwoFactorAuthenticatable, TwoFactorState,
    },
    core::BoxFuture,
};

/// 当前应用账户模型；密码列沿用已有存储字段，其他应用可实现自己的 `Authenticatable`。
pub struct User {
    attributes: GenericUser,
    #[cfg(feature = "auth-fortify")]
    two_factor_state: Option<DatabaseTwoFactorState>,
    #[cfg(feature = "auth-fortify")]
    factor_authority: Option<DatabaseFactorAuthority>,
}
impl User {
    /// 从数据库Provider已转换的属性构造账户，不要求邮箱或手机号存在。
    pub fn new(attributes: Map<String, Value>) -> Self {
        Self {
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
impl Authenticatable for User {
    fn auth_attribute(&self, name: &str) -> Result<Option<Value>> {
        self.attributes.attribute(name)
    }

    #[cfg(feature = "auth-fortify")]
    fn as_two_factor(&self) -> Option<&dyn TwoFactorAuthenticatable> {
        self.two_factor_state
            .as_ref()
            .map(|_| self as &dyn TwoFactorAuthenticatable)
    }
    fn auth_identifier_name(&self) -> &'static str {
        "id"
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.attributes.auth_identifier()
    }
    fn auth_password_name(&self) -> &'static str {
        "password_hash"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        match self.attributes.attribute("password_hash")? {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value)),
            _ => Err(openrust::Error::custom(
                "app_password_hash_invalid",
                "应用账户密码哈希类型无效",
            )),
        }
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.attributes.remember_token()
    }
    fn set_remember_token(&self, token: Option<String>) -> Result<()> {
        self.attributes.set_remember_token(token)
    }
    fn compare_and_set_remember_token(
        &self,
        expected: Option<&str>,
        replacement: Option<String>,
    ) -> Result<bool> {
        self.attributes
            .compare_and_set_remember_token(expected, replacement)
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

#[cfg(all(test, feature = "auth-fortify"))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
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
            let user = User::new(attributes.as_object().cloned().unwrap_or_default());
            assert_eq!(
                user.two_factor_account_name("account")?.as_deref(),
                Some(expected)
            );
        }
        Ok(())
    }
}

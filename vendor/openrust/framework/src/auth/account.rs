//! 不依赖认证运行时的共享账户契约与通用属性账户。

use std::{fmt, sync::Arc};

use serde_json::{Map, Value};

use crate::{Error, Result, support::BoxFuture};

#[path = "guard/remember.rs"]
mod remember;
pub use remember::{RememberTokenMutation, RememberTokenUpdate};

/// Provider 返回的共享账户；缓存必须属于请求，不能放入进程级已登录状态。
/// 启用 `auth` 后可通过容器解析 `Option<AuthUser>` 获取当前默认 Guard 的可空账户。
pub type AuthUser = Arc<dyn Authenticatable>;

/// 不绑定数据库表、邮箱字段或整数主键的认证账户。
///
/// 异步认证调用链共享同一账户对象，故记住我 token
/// 通过内部可变性修改；实现不得在跨越 await 时保留同步锁。
pub trait Authenticatable: Send + Sync + 'static {
    /// 返回同一账户对象的可选持久模型身份；不能用登录标识猜测多态主键。
    fn as_model(&self) -> Option<&dyn crate::database::eloquent::ModelIdentity> {
        None
    }
    /// 返回同一账户对象的可选Fortify能力；普通账户无需实现双因素状态存储。
    #[cfg(feature = "auth-fortify")]
    fn as_two_factor(&self) -> Option<&dyn crate::auth::fortify::TwoFactorAuthenticatable> {
        None
    }
    /// 读取模型登录字段，供密码确认复用同一Guard查询，不以邮箱为固定前提。
    fn auth_attribute(&self, _name: &str) -> Result<Option<Value>> {
        Err(Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "auth_account_attribute_unsupported",
            "当前账户模型未实现公开属性读取",
        ))
    }
    /// 返回账户标识字段名。
    fn auth_identifier_name(&self) -> &str;
    /// 返回账户标识，保留字符串及数字的原始 JSON 类型。
    fn auth_identifier(&self) -> Result<Value>;
    /// 返回密码哈希字段名。
    fn auth_password_name(&self) -> &str;
    /// 返回已存储哈希；无密码账户返回 None，不得将其当作空密码。
    fn auth_password(&self) -> Result<Option<String>>;
    /// 返回持久化记住我 token。
    fn remember_token(&self) -> Result<Option<String>>;
    /// 返回可用于签发Cookie的已确认token快照，不能暴露另一Guard尚未持久化的值。
    ///
    /// 此读取须原子核验未提交状态与实际属性值；不支持的外部账户明确拒绝，
    /// 不能用一次独立检查后再读取可变属性的方式实现。
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        Err(Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "auth_committed_token_read_unsupported",
            "账户未提供已确认的记住我属性读取",
        ))
    }
    /// 经账户自身的属性赋值机制更新token；Provider负责持久化。
    fn set_remember_token(&self, token: Option<String>) -> BoxFuture<'_, Result<()>>;
    /// 条件准备并原子安装记住我属性修改；false条件返回None，成功返回拥有式回执。
    ///
    /// setter可异步，但返回回执之前必须已安装原始补丁并armed同步恢复；
    /// Drop不得调用用户访问器或异步恢复。外部账户通过公开mutation后端实现。
    fn begin_remember_token_update<'a>(
        &'a self,
        _expected: Option<&'a str>,
        _replacement: Option<String>,
    ) -> BoxFuture<'a, Result<Option<RememberTokenUpdate>>> {
        Box::pin(async {
            Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "auth_token_atomic_update_unsupported",
                "账户未提供可恢复的记住我属性修改",
            ))
        })
    }
    /// 返回记住我 token 字段名。
    fn remember_token_name(&self) -> &str;
}

/// 通用属性账户，不依赖具体 ORM 或业务模型。
///
/// 字段名固定为 `id/password/remember_token`；需要其他字段名时实现 `Authenticatable`。
/// Debug有意不包含任何属性值；内存属性更改不会隐式写数据库。
pub struct GenericUser {
    attributes: crate::database::eloquent::ModelAttributes,
}

impl GenericUser {
    /// 保留 Provider 返回的属性原始类型。
    pub fn new(attributes: Map<String, Value>) -> Self {
        Self {
            attributes: crate::database::eloquent::ModelAttributes::new(attributes),
        }
    }
    /// 返回单个属性副本；调用方负责保护敏感值。
    pub fn attribute(&self, name: &str) -> Result<Option<Value>> {
        self.attributes
            .raw_attribute(name)
            .map_err(auth_attribute_error)
    }
    /// 更新内存属性，供用户 Provider 在持久化成功后同步对象。
    pub fn set_attribute(&self, name: impl Into<String>, value: Value) -> Result<()> {
        self.attributes
            .set_raw_attribute(name, value)
            .map_err(auth_attribute_error)
    }
    /// 返回同一账户的属性存储，供应用模型执行转换；不复制认证或记住我状态。
    pub fn model_attributes(&self) -> &crate::database::eloquent::ModelAttributes {
        &self.attributes
    }
    fn string_attribute(&self, name: &str) -> Result<Option<String>> {
        Self::string_value(self.attribute(name)?)
    }
    fn string_value(value: Option<Value>) -> Result<Option<String>> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value)),
            _ => Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "auth_user_attribute_invalid",
                "认证账户秘密字段必须是字符串或null",
            )),
        }
    }
}

impl fmt::Debug for GenericUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GenericUser([REDACTED])")
    }
}

impl Authenticatable for GenericUser {
    fn auth_attribute(&self, name: &str) -> Result<Option<Value>> {
        self.attribute(name)
    }

    fn auth_identifier_name(&self) -> &'static str {
        "id"
    }
    fn auth_identifier(&self) -> Result<Value> {
        Ok(self.attribute("id")?.unwrap_or(Value::Null))
    }
    fn auth_password_name(&self) -> &'static str {
        "password"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        self.string_attribute("password")
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.string_attribute("remember_token")
    }
    fn remember_token_for_cookie(&self) -> Result<Option<String>> {
        Self::string_value(
            self.attributes
                .committed_raw_attribute("remember_token")
                .map_err(auth_attribute_error)?,
        )
    }
    fn set_remember_token(&self, token: Option<String>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.set_attribute("remember_token", token.map_or(Value::Null, Value::String))
        })
    }
    fn begin_remember_token_update<'a>(
        &'a self,
        expected: Option<&'a str>,
        replacement: Option<String>,
    ) -> BoxFuture<'a, Result<Option<RememberTokenUpdate>>> {
        Box::pin(async move {
            self.attributes
                .set_raw_attribute_reversibly_if(
                    "remember_token",
                    replacement.map_or(Value::Null, Value::String),
                    |current| {
                        let current = match current {
                            None | Some(Value::Null) => None,
                            Some(Value::String(token)) => Some(token),
                            Some(_) => {
                                return Err(Error::custom_with_status(
                                    http::StatusCode::INTERNAL_SERVER_ERROR,
                                    "auth_user_attribute_invalid",
                                    "账户token属性类型无效",
                                ));
                            }
                        };
                        Ok(current.as_deref() == expected)
                    },
                )
                .map(|update| update.map(RememberTokenUpdate::from))
                .map_err(auth_attribute_error)
        })
    }
    fn remember_token_name(&self) -> &'static str {
        "remember_token"
    }
}

fn user_lock_error() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "auth_user_state_poisoned",
        "认证账户状态不可用",
    )
}

fn auth_attribute_error(error: Error) -> Error {
    if error.code() == "model_attribute_state_unavailable" {
        user_lock_error()
    } else {
        error
    }
}

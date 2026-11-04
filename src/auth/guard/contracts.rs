//! 具名认证 Guard 与用户 Provider 的公开异步契约。

use std::{
    fmt,
    sync::{Arc, RwLock},
};

use serde_json::{Map, Value};

use crate::{Error, Result, core::BoxFuture};

/// Provider 返回的共享账户；缓存必须属于请求，不能放入进程级已登录状态。
pub type AuthUser = Arc<dyn Authenticatable>;

/// 不绑定数据库表、邮箱字段或整数主键的认证账户。
///
/// 对应 Laravel Authenticatable。异步调用链共享同一账户对象，故记住我 token
/// 通过内部可变性修改；实现不得在跨越 await 时保留同步锁。
pub trait Authenticatable: Send + Sync + 'static {
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
    /// 更新当前账户对象中的记住我 token；存储更新由 `UserProvider` 负责。
    fn set_remember_token(&self, token: Option<String>) -> Result<()>;
    /// 在同一账户锁内比较并替换 token；成功返回 true，已变化返回 false。
    ///
    /// Rust 异步取消恢复需要原子能力；未实现时在修改前拒绝，不能用读取再写入模拟。
    fn compare_and_set_remember_token(
        &self,
        _expected: Option<&str>,
        _replacement: Option<String>,
    ) -> Result<bool> {
        Err(Error::custom(
            "auth_token_atomic_update_unsupported",
            "账户未提供原子记住我token更新",
        ))
    }
    /// 返回记住我 token 字段名。
    fn remember_token_name(&self) -> &str;
}

/// 账户检索与密码验证输入；调试输出不泄露任意字段。
#[derive(Clone, Default)]
pub struct Credentials(Map<String, Value>);

impl Credentials {
    /// 从调用方明确提供的字段创建凭据，不强制 email 或额外字段。
    pub fn new(attributes: Map<String, Value>) -> Self {
        Self(attributes)
    }

    /// 返回指定原始字段；密码等秘密只应交给 Provider 验证。
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    /// 返回全部字段，供外部 Provider 实现同一查询契约。
    pub fn attributes(&self) -> &Map<String, Value> {
        &self.0
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credentials([REDACTED])")
    }
}

/// Guard 使用的账户来源；方法与 Laravel `UserProvider` 职责逐项对应。
///
/// 检索密码凭据不得建立正式登录状态。实现应只在 `rehash_password_if_required`
/// 更新密码哈希，不能用未知账户、缺失密码或存储失败返回成功。
pub trait UserProvider: Send + Sync + 'static {
    /// 显式证明既有数据库账户流程可使用此来源；默认不承诺兼容。
    ///
    /// 此 Rust 适配能力只用于把已绑定数据库账户域的 AuthFlows/OIDC 桥接到 Guard，
    /// 必须同时核同一连接句柄和实际账户表，不能仅比较 Provider 名或整数 ID。
    fn uses_database_accounts(&self, _db: &crate::db::Db, _table: &str) -> Result<bool> {
        Ok(false)
    }
    /// 按原始标识查找账户；找不到时返回 None。
    fn retrieve_by_id<'a>(
        &'a self,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>>;
    /// 使用标识与持久化记住我 token 查找账户。
    fn retrieve_by_token<'a>(
        &'a self,
        identifier: &'a Value,
        token: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>>;
    /// 持久化记住我 token；调用方同步更新账户对象。
    fn update_remember_token<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        token: &'a str,
    ) -> BoxFuture<'a, Result<()>>;
    /// 按凭据的非密码查询条件检索，不在此方法校验密码或建立登录状态。
    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>>;
    /// 验证已取到账户的密码等凭据；不改变登录状态。
    fn validate_credentials<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>>;
    /// 按当前哈希策略重算；force=true时强制更新，不默认静默省略。
    fn rehash_password_if_required<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
        force: bool,
    ) -> BoxFuture<'a, Result<()>>;
}

/// 请求内的认证状态与凭据验证接口。
pub trait Guard: Send + Sync + 'static {
    /// 返回实际选择的具名账户Provider；无Provider的custom driver返回None。
    fn provider_name(&self) -> Option<&str> {
        None
    }
    /// 返回当前账户；缺失或未认证时为 None。
    fn user(&self) -> BoxFuture<'_, Result<Option<AuthUser>>>;
    /// 只验证凭据，不写 Session、Cookie 或已登录用户缓存。
    fn validate<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, Result<bool>>;
    /// 返回是否已缓存用户，不触发 Provider 查询。
    fn has_user(&self) -> Result<bool>;
    /// 设置当前请求用户，不写持久 Session 或 Cookie。
    fn set_user(&self, user: AuthUser) -> BoxFuture<'_, Result<()>>;
    /// 返回可用的有状态契约；token等无状态driver默认不提供。
    fn as_stateful(&self) -> Option<&dyn StatefulGuard> {
        None
    }

    /// 返回当前是否认证；存储失败不能变成 guest 成功。
    fn check(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { Ok(self.user().await?.is_some()) })
    }
    /// 返回当前是否为访客。
    fn guest(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async move { Ok(!self.check().await?) })
    }
    /// 返回当前用户原始标识，不强制转换为整数。
    fn id(&self) -> BoxFuture<'_, Result<Option<Value>>> {
        Box::pin(async move {
            self.user()
                .await?
                .map(|user| user.auth_identifier())
                .transpose()
        })
    }
}

/// Session Guard 的登录、单次认证和注销行为。
pub trait StatefulGuard: Guard {
    /// 该 Guard 的持久 Session 身份键；非 Session 实现默认不提供此能力。
    fn session_name(&self) -> Option<String> {
        None
    }
    /// 返回本 Guard 所用账户 Provider，供 Fortify 首因子验证复用。
    fn provider(&self) -> Arc<dyn UserProvider>;
    /// 验证凭据并登录；remember控制持久记住我 Cookie。
    fn attempt<'a>(
        &'a self,
        credentials: &'a Credentials,
        remember: bool,
    ) -> BoxFuture<'a, Result<bool>>;
    /// 仅在当前请求认证，不写 Session 或 Cookie。
    fn once<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, Result<bool>>;
    /// 登录可信调用方提供的账户并轮换 Session；Fortify须在MFA完成后才调用。
    fn login(&self, user: AuthUser, remember: bool) -> BoxFuture<'_, Result<()>>;
    /// 按 Provider 标识查找并登录，缺失账户返回 None。
    fn login_using_id<'a>(
        &'a self,
        identifier: &'a Value,
        remember: bool,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>>;
    /// 仅按标识建立本请求身份，缺失账户返回 None。
    fn once_using_id<'a>(
        &'a self,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>>;
    /// 返回本请求是否从记住我 Cookie 恢复。
    fn via_remember(&self) -> Result<bool>;
    /// 删除本 Guard 登录状态、撤销记住我 token并清用户缓存。
    fn logout(&self) -> BoxFuture<'_, Result<()>>;
}

/// Laravel `GenericUser` 对应的属性账户，不依赖具体 ORM 或业务模型。
///
/// 字段名固定为 `id/password/remember_token`；需要其他字段名时实现 `Authenticatable`。
/// Debug有意不包含任何属性值；内存属性更改不会隐式写数据库。
pub struct GenericUser {
    attributes: RwLock<Map<String, Value>>,
}

impl GenericUser {
    /// 保留 Provider 返回的属性原始类型。
    pub fn new(attributes: Map<String, Value>) -> Self {
        Self {
            attributes: RwLock::new(attributes),
        }
    }
    /// 返回单个属性副本；调用方负责保护敏感值。
    pub fn attribute(&self, name: &str) -> Result<Option<Value>> {
        let values = self.attributes.read().map_err(|_| user_lock_error())?;
        Ok(values.get(name).cloned())
    }
    /// 更新内存属性，供用户 Provider 在持久化成功后同步对象。
    pub fn set_attribute(&self, name: impl Into<String>, value: Value) -> Result<()> {
        self.attributes
            .write()
            .map_err(|_| user_lock_error())?
            .insert(name.into(), value);
        Ok(())
    }
    fn string_attribute(&self, name: &str) -> Result<Option<String>> {
        match self.attribute(name)? {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value)),
            _ => Err(Error::custom(
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
    fn set_remember_token(&self, token: Option<String>) -> Result<()> {
        self.set_attribute("remember_token", token.map_or(Value::Null, Value::String))
    }
    fn compare_and_set_remember_token(
        &self,
        expected: Option<&str>,
        replacement: Option<String>,
    ) -> Result<bool> {
        let mut attributes = self.attributes.write().map_err(|_| user_lock_error())?;
        let current = match attributes.get("remember_token") {
            None | Some(Value::Null) => None,
            Some(Value::String(token)) => Some(token.as_str()),
            Some(_) => {
                return Err(Error::custom(
                    "auth_user_attribute_invalid",
                    "账户token属性类型无效",
                ));
            }
        };
        if current != expected {
            return Ok(false);
        }
        attributes.insert(
            "remember_token".into(),
            replacement.map_or(Value::Null, Value::String),
        );
        Ok(true)
    }
    fn remember_token_name(&self) -> &'static str {
        "remember_token"
    }
}

fn user_lock_error() -> Error {
    Error::custom("auth_user_state_poisoned", "认证账户状态不可用")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn generic_users_preserve_identifier_types_and_hide_all_attributes() -> Result<()> {
        for identifier in [json!("tenant:user-42"), json!(42), json!(u64::MAX)] {
            let user = GenericUser::new(Map::from_iter([
                ("id".into(), identifier.clone()),
                ("password".into(), json!("sensitive-hash")),
                ("remember_token".into(), json!("sensitive-token")),
            ]));
            assert_eq!(user.auth_identifier()?, identifier);
            assert_eq!(user.auth_password()?.as_deref(), Some("sensitive-hash"));
            assert_eq!(format!("{user:?}"), "GenericUser([REDACTED])");
            user.set_remember_token(None)?;
            assert_eq!(user.remember_token()?, None);
            user.set_remember_token(Some("replacement".into()))?;
            assert_eq!(user.remember_token()?.as_deref(), Some("replacement"));
        }
        Ok(())
    }

    #[test]
    fn missing_password_is_not_coerced_to_empty_and_invalid_secret_types_fail() -> Result<()> {
        let user = GenericUser::new(Map::new());
        assert_eq!(user.auth_password()?, None);
        user.set_attribute("password", json!(false))?;
        assert!(user.auth_password().is_err());
        let credentials = Credentials::new(Map::from_iter([
            ("username".into(), json!("private-user")),
            ("password".into(), json!("private-password")),
        ]));
        assert_eq!(format!("{credentials:?}"), "Credentials([REDACTED])");
        assert_eq!(credentials.get("username"), Some(&json!("private-user")));
        Ok(())
    }
}

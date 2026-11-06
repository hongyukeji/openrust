//! 具名认证 Guard 与用户 Provider 的公开异步契约。

use std::{fmt, sync::Arc};

use serde_json::{Map, Value};

use crate::{Error, Result, core::BoxFuture};

/// Provider 返回的共享账户；缓存必须属于请求，不能放入进程级已登录状态。
pub type AuthUser = Arc<dyn Authenticatable>;

/// 不绑定数据库表、邮箱字段或整数主键的认证账户。
///
/// 对应 Laravel Authenticatable。异步调用链共享同一账户对象，故记住我 token
/// 通过内部可变性修改；实现不得在跨越 await 时保留同步锁。
pub trait Authenticatable: Send + Sync + 'static {
    /// 返回同一账户对象的可选持久模型身份；不能用登录标识猜测多态主键。
    fn as_model(&self) -> Option<&dyn crate::db::eloquent::ModelIdentity> {
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
    ) -> BoxFuture<'a, Result<Option<super::RememberTokenUpdate>>> {
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
    /// 是否支持将 Fortify 登录与额外 Session 轮换放入同一失败隔离范围。
    ///
    /// 构造 Fortify 登录服务时先检查此能力，默认在密码验证或重哈希前拒绝。
    fn supports_session_regeneration_login(&self) -> bool {
        false
    }
    /// 该 Guard 的持久 Session 身份键；非 Session 实现默认不提供此能力。
    fn session_name(&self) -> Option<String> {
        None
    }
    /// 返回实际 Guard 名称与请求 Session。自定义 Guard 可选择此扩展，凭据派生会独立复核当前用户、Provider 和已保存记录。
    #[cfg(feature = "auth-fortify")]
    fn credential_parent_session(&self) -> Option<(&str, crate::session::Session)> {
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
    /// 验证凭据、登录并完成 Fortify 的额外 Session 轮换，全部属于同一失败隔离范围。
    ///
    /// 自定义 Guard 必须显式提供组合实现；默认在任何认证或存储变更前返回 500。
    fn attempt_with_session_regeneration<'a>(
        &'a self,
        _credentials: &'a Credentials,
        _remember: bool,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Err(combined_regeneration_unsupported()) })
    }
    /// 仅在当前请求认证，不写 Session 或 Cookie。
    fn once<'a>(&'a self, credentials: &'a Credentials) -> BoxFuture<'a, Result<bool>>;
    /// 登录可信调用方提供的账户并轮换 Session；Fortify须在MFA完成后才调用。
    fn login(&self, user: AuthUser, remember: bool) -> BoxFuture<'_, Result<()>>;
    /// 登录可信账户并完成 Fortify 的额外 Session 轮换，不在两个 await 之间提前提交身份。
    ///
    /// 自定义 Guard 必须显式提供组合实现；默认在任何认证或存储变更前返回 500。
    fn login_with_session_regeneration(
        &self,
        _user: AuthUser,
        _remember: bool,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Err(combined_regeneration_unsupported()) })
    }
    /// 使用内部因子收据登录，并在同一失败隔离范围完成额外 Session 轮换。
    fn login_with_factor_receipt(
        &self,
        _user: AuthUser,
        _remember: bool,
        _receipt: super::FactorReceipt,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "auth_guard_factor_receipt_unsupported",
                "当前认证 Guard 尚未实现因子收据登录",
            ))
        })
    }
    /// 读取同一持久身份的因子快照；调用方仍须核验 authority 当前来源与代际。
    fn session_factor_proof(&self) -> BoxFuture<'_, Result<Option<super::SessionFactorProof>>> {
        Box::pin(async { Ok(None) })
    }
    /// 将已提交的实际因子验证附加到当前持久身份；不重新登录或额外轮换Session。
    ///
    /// 用于已登录用户确认绑定的连续流程，收据仍只能由受控验证路径签发。
    fn attest_factor_receipt(&self, _receipt: super::FactorReceipt) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "auth_guard_factor_attestation_unsupported",
                "当前认证Guard尚未实现当前身份因子确认",
            ))
        })
    }
    /// 核验当前持久身份，取得受限管理上下文；它不授权一般受保护资源。
    fn factor_management_context(&self) -> BoxFuture<'_, Result<super::FactorManagementContext>> {
        #[cfg(feature = "auth-fortify")]
        if let Some((name, session)) = self.credential_parent_session() {
            return Box::pin(async move {
                crate::auth::credential::persisted_guard_management(self, name, &session).await
            });
        }
        Box::pin(async {
            Err(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "auth_guard_factor_management_unsupported",
                "当前认证Guard尚未实现因子管理上下文",
            ))
        })
    }
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

fn combined_regeneration_unsupported() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "auth_guard_session_regeneration_unsupported",
        "当前认证 Guard 尚未实现登录与额外 Session 轮换的组合操作",
    )
}

/// Laravel `GenericUser` 对应的属性账户，不依赖具体 ORM 或业务模型。
///
/// 字段名固定为 `id/password/remember_token`；需要其他字段名时实现 `Authenticatable`。
/// Debug有意不包含任何属性值；内存属性更改不会隐式写数据库。
pub struct GenericUser {
    attributes: crate::db::eloquent::ModelAttributes,
}

impl GenericUser {
    /// 保留 Provider 返回的属性原始类型。
    pub fn new(attributes: Map<String, Value>) -> Self {
        Self {
            attributes: crate::db::eloquent::ModelAttributes::new(attributes),
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
    pub fn model_attributes(&self) -> &crate::db::eloquent::ModelAttributes {
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
    ) -> BoxFuture<'a, Result<Option<super::RememberTokenUpdate>>> {
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
                .map(|update| update.map(super::RememberTokenUpdate::from))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn model_attributes_and_authentication_share_one_raw_store() -> Result<()> {
        let user = GenericUser::new(Map::from_iter([("id".into(), json!("00042"))]));
        let metadata = crate::db::eloquent::ModelMetadata::new("users", "User");
        assert_eq!(
            user.model_attributes().get_attribute(&metadata, "id")?,
            Some(json!(42))
        );
        assert_eq!(user.auth_identifier()?, json!("00042"));
        user.model_attributes()
            .set_raw_attribute("remember_token", json!("first"))?;
        assert_eq!(user.remember_token()?.as_deref(), Some("first"));
        user.begin_remember_token_update(Some("first"), Some("next".into()))
            .await?
            .ok_or_else(|| Error::custom("test_token_update_missing", "应取得token修改回执"))?
            .commit()?;
        assert_eq!(
            user.model_attributes().raw_attribute("remember_token")?,
            Some(json!("next"))
        );
        assert!(
            user.begin_remember_token_update(Some("first"), None)
                .await?
                .is_none()
        );
        Ok(())
    }

    #[tokio::test]
    async fn generic_users_preserve_identifier_types_and_hide_all_attributes() -> Result<()> {
        for identifier in [json!("tenant:user-42"), json!(42), json!(u64::MAX)] {
            let user = GenericUser::new(Map::from_iter([
                ("id".into(), identifier.clone()),
                ("password".into(), json!("sensitive-hash")),
                ("remember_token".into(), json!("sensitive-token")),
            ]));
            assert_eq!(user.auth_identifier()?, identifier);
            assert_eq!(user.auth_password()?.as_deref(), Some("sensitive-hash"));
            assert_eq!(format!("{user:?}"), "GenericUser([REDACTED])");
            user.set_remember_token(None).await?;
            assert_eq!(user.remember_token()?, None);
            user.set_remember_token(Some("replacement".into())).await?;
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

//! Passport v13.10.0 Bridge Client 的纯协议投影；不查询、缓存或持久化客户端。
//!
//! 对应 `Bridge/Client.php`、`Bridge/ClientRepository.php::fromClientModel`，
//! 以及 League 9.4.1 的 `ClientTrait` / `EntityTrait`。

use super::{ClientModel, PhpArray};
use openrust::{Error, Result};
use serde_json::Value;

/// OAuth 协议使用的客户端实体；与保留原始主键的持久模型分别承担职责。
///
/// 这里只保存协议字段，不携带模型、编码 secret 或临时明文 secret。
///
/// ```
/// use openrust_passport::BridgeClient;
///
/// let client = BridgeClient::new(
///     "consumer", Some("应用".into()), vec![], false, None,
///     vec!["authorization_code".into()],
/// );
/// assert_eq!(client.get_identifier(), "consumer");
/// assert!(client.supports_grant_type("authorization_code"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BridgeClient {
    identifier: String,
    name: Option<String>,
    redirect_uri: PhpArray,
    confidential: bool,
    /// 对应官方公开的可选认证用户 Provider 名称。
    pub provider: Option<String>,
    /// 对应官方公开的允许 grant 列表；保留顺序及重复项。
    pub grant_types: PhpArray,
}

impl BridgeClient {
    /// 显式构造协议实体；参数默认值由调用方以 None、空列表和 false 表达。
    ///
    /// 与固定构造器相同，空 identifier 不在此额外校验；合法客户端选择属于仓储。
    /// name 为 None 时保留官方未初始化名称的状态，由读取名称时返回错误。
    #[must_use]
    pub fn new(
        identifier: impl Into<String>,
        name: Option<String>,
        redirect_uri: Vec<String>,
        confidential: bool,
        provider: Option<String>,
        grant_types: Vec<String>,
    ) -> Self {
        Self::new_with_arrays(
            identifier,
            name,
            redirect_uri.into(),
            confidential,
            provider,
            grant_types.into(),
        )
    }

    /// 从保留关联键与异质值的数组构造；URI 成员判断仍须采用严格字符串比较。
    #[must_use]
    pub fn new_with_arrays(
        identifier: impl Into<String>,
        name: Option<String>,
        redirect_uri: PhpArray,
        confidential: bool,
        provider: Option<String>,
        grant_types: PhpArray,
    ) -> Self {
        Self {
            identifier: identifier.into(),
            name,
            redirect_uri,
            confidential,
            provider,
            grant_types,
        }
    }

    /// 从所选客户端模型的公开属性能力创建独立协议快照。
    ///
    /// 主键仅在协议边界转为字符串，Rust 原生整数精确输出十进制，不经过浮点数。
    /// 超出 PHP 原生有符号整数范围的 u64 是 Rust 载体适配，不扩大默认数据库键域。
    /// 不在此筛选 revoked、校验 secret 或决定 grant 资格；这些由仓储和协议流程负责。
    /// 模型读取失败原样传播；浮点及其他标量沿用 PHP 字符串转换，复合主键明确失败。
    pub fn from_client_model(model: &dyn ClientModel) -> Result<Self> {
        Ok(Self::new_with_arrays(
            protocol_identifier(model.model_key()?)?,
            model.name()?,
            model.redirect_uris()?,
            model.confidential()?,
            model.provider()?,
            model.grant_types()?,
        ))
    }

    /// 返回协议边界的字符串标识符。
    #[must_use]
    pub fn get_identifier(&self) -> &str {
        &self.identifier
    }

    /// 替换协议标识符；不修改持久模型主键。
    pub fn set_identifier(&mut self, identifier: impl Into<String>) {
        self.identifier = identifier.into();
    }

    /// 返回客户端显示名称；官方未初始化的名称在此表现为框架错误。
    pub fn get_name(&self) -> Result<&str> {
        self.name.as_deref().ok_or_else(|| {
            Error::custom_with_status(
                openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "passport_bridge_client_name_uninitialized",
                "客户端协议名称尚未初始化",
            )
        })
    }

    /// 返回注册的 redirect URI 列表，保留空片段、顺序及重复项。
    #[must_use]
    pub fn get_redirect_uri(&self) -> &PhpArray {
        &self.redirect_uri
    }

    /// 返回模型投影时的保密客户端状态。
    #[must_use]
    pub fn is_confidential(&self) -> bool {
        self.confidential
    }

    /// 按固定 Passport 的非严格 `in_array` 字符串语义判断 grant。
    #[must_use]
    pub fn supports_grant_type(&self, grant_type: &str) -> bool {
        self.grant_types.contains_loose(grant_type)
    }
}

fn protocol_identifier(value: Value) -> Result<String> {
    // PHP 无原生 u64；Rust 整数载体必须避免经浮点投影碰撞，不改全局 PHP cast。
    if let Value::Number(number) = &value
        && (number.is_i64() || number.is_u64())
    {
        return Ok(number.to_string());
    }
    if value.is_array() || value.is_object() {
        return Err(invalid_protocol_identifier());
    }
    match openrust::database::eloquent::AttributeCast::String.read(Some(value))? {
        Some(Value::String(value)) => Ok(value),
        _ => Err(invalid_protocol_identifier()),
    }
}
fn invalid_protocol_identifier() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "passport_bridge_client_identifier_invalid",
        "客户端协议标识必须可转换为字符串",
    )
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;

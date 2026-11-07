//! Passport 客户端纯模型；来源 Laravel Passport v13.10.0 Client.php（MIT）。

use super::{PhpArray, Scope, ScopeRepository};
use openrust::{
    Error, Result,
    auth::Authenticatable,
    database::eloquent::{
        AttributeCast, AttributeGetter, AttributeSetter, ModelAttributes, ModelIdentity,
        ModelMetadata, casts::cast_attribute_as_hashed_string,
    },
    hashing::HashManager,
    support::BoxFuture,
};
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};
use std::sync::{Arc, RwLock};

/// 可替换的 Passport 客户端模型；快照保留数据库原值，属性转换不执行关系查询。
pub trait ClientModel: ModelIdentity {
    /// 读取原始持久属性快照，不包含临时明文 secret。
    fn attributes(&self) -> Result<Map<String, Value>>;
    /// 读取模型属性；默认快照实现兼容外部模型，具备存储的模型应覆写以应用访问器。
    ///
    /// redirect/grant 保留固定 Passport 的内置访问器；不会创建第二份属性存储。
    fn get_attribute(&self, name: &str) -> Result<Option<Value>> {
        let attributes = self.attributes()?;
        let raw = attributes.get(name);
        match name {
            "redirect_uris" => redirect_attribute(raw, &attributes),
            "grant_types" => grant_attribute(self, raw),
            "plain_secret" => self
                .plain_secret()
                .map(|value| Some(value.map_or(Value::Null, Value::String))),
            _ => match self
                .model_metadata()
                .effective_cast(name)?
                .or_else(|| client_cast(name))
            {
                Some(cast) => cast.read(raw.cloned()),
                None => Ok(raw.cloned()),
            },
        }
    }
    /// 水合或成功持久化后回写原始属性；不得重新哈希 secret。
    fn set_raw_attributes(&self, attributes: Map<String, Value>) -> Result<()>;
    /// 显式读取原始编码 secret；不会出现在 JSON 中。
    fn secret(&self) -> Result<Option<String>> {
        optional_string(self.attributes()?.get("secret"))
    }
    /// 返回本次赋值请求的临时明文；水合模型没有该值。
    fn plain_secret(&self) -> Result<Option<String>>;
    /// 自定义setter优先；仅默认分支记录临时输入并经同一公开hashed cast。
    fn set_secret<'a>(
        &'a self,
        hashing: &'a HashManager,
        secret: Option<&'a str>,
    ) -> BoxFuture<'a, Result<()>>;
    /// 客户端显示名称。
    fn name(&self) -> Result<Option<String>> {
        optional_string(self.get_attribute("name")?.as_ref())
    }
    /// 可选认证用户 Provider 名称。
    fn provider(&self) -> Result<Option<String>> {
        optional_string(self.get_attribute("provider")?.as_ref())
    }
    /// 原始非empty JSON或显式已解码数组优先，否则按逗号拆分旧redirect。
    fn redirect_uris(&self) -> Result<PhpArray> {
        PhpArray::try_from(
            self.get_attribute("redirect_uris")?
                .ok_or_else(invalid_attribute)?,
        )
    }
    /// 非 null 新字段优先，否则按官方顺序计算旧模型允许的七类 grant。
    fn grant_types(&self) -> Result<PhpArray> {
        PhpArray::try_from(
            self.get_attribute("grant_types")?
                .ok_or_else(invalid_attribute)?,
        )
    }
    /// `user_id` 字段存在时优先于 `owner_id`，沿用 PHP empty 语义。
    fn first_party(&self) -> Result<bool> {
        let raw = self.attributes()?;
        let field = if raw.contains_key("user_id") {
            "user_id"
        } else {
            "owner_id"
        };
        Ok(!truthy(self.get_attribute(field)?.as_ref()))
    }
    /// 原始 secret 非空才是保密客户端。
    fn confidential(&self) -> Result<bool> {
        Ok(truthy(self.attributes()?.get("secret")))
    }
    /// 客户端撤销状态，沿用 Eloquent bool 转换。
    fn revoked(&self) -> Result<bool> {
        Ok(truthy(self.get_attribute("revoked")?.as_ref()))
    }
    /// 是否允许指定 grant。
    fn has_grant_type(&self, grant: &str) -> Result<bool> {
        Ok(self.grant_types()?.contains_loose(grant))
    }
    /// 使用同一 scope 仓储判断客户端限制；缺失或 null 不限制，空数组拒绝全部。
    fn has_scope(&self, scopes: &ScopeRepository, scope: &str) -> Result<bool> {
        let attributes = self.attributes()?;
        let restrictions = match attributes.get("scopes") {
            None | Some(Value::Null) => None,
            Some(_) => Some(PhpArray::try_from(
                self.get_attribute("scopes")?
                    .ok_or_else(invalid_attribute)?,
            )?),
        };
        Ok(scopes.client_has_scope(restrictions.as_ref(), scope))
    }
    /// 默认每次显示授权提示；应用模型可显式覆写同意策略。
    fn skips_authorization(&self, _user: &dyn Authenticatable, _scopes: &[Scope]) -> Result<bool> {
        Ok(false)
    }
    /// 共同元数据可见性过滤及官方数组、布尔转换后的安全 JSON 属性。
    fn arrayable_attributes(&self) -> Result<Map<String, Value>> {
        let raw = self.attributes()?;
        let mut values = self.model_metadata().arrayable_attributes(&raw);
        values.remove("secret");
        values.remove("plain_secret");
        values.remove("plainSecret");
        for (key, value) in &mut values {
            let projected = self.get_attribute(key)?.unwrap_or(Value::Null);
            *value = match (key.as_str(), projected) {
                (
                    "scopes" | "grant_types" | "redirect_uris",
                    array @ (Value::Array(_) | Value::Object(_)),
                ) => PhpArray::try_from(array)?.into(),
                (_, value) => value,
            };
        }
        Ok(values)
    }
}

/// 默认 Passport 客户端；同步短锁只保护快照，不跨哈希工作持锁。
pub struct Client {
    metadata: Arc<ModelMetadata>,
    attributes: ModelAttributes,
    plain_secret: RwLock<Option<String>>,
}
impl Client {
    /// 水合原始属性；不哈希、不恢复临时明文。
    pub fn from_attributes(metadata: Arc<ModelMetadata>, attributes: Map<String, Value>) -> Self {
        Self {
            metadata: with_client_casts(metadata),
            attributes: ModelAttributes::new(attributes),
            plain_secret: RwLock::new(None),
        }
    }
    /// 同一原始属性存储，供应用登记访问器；不复制客户端数据。
    pub fn model_attributes(&self) -> &ModelAttributes {
        &self.attributes
    }
}
impl ModelIdentity for Client {
    fn model_metadata(&self) -> &ModelMetadata {
        &self.metadata
    }
    fn model_key(&self) -> Result<Value> {
        Ok(self
            .attributes
            .get_attribute(&self.metadata, &self.metadata.key_name)?
            .unwrap_or(Value::Null))
    }
}
impl ClientModel for Client {
    fn get_attribute(&self, name: &str) -> Result<Option<Value>> {
        match name {
            "redirect_uris" | "grant_types" | "plain_secret" => {
                self.attributes.get_attribute_with_default_getter(
                    &self.metadata,
                    name,
                    &ClientGetter { client: self, name },
                )
            }
            _ => self.attributes.get_attribute(&self.metadata, name),
        }
    }
    fn attributes(&self) -> Result<Map<String, Value>> {
        self.attributes.raw_attributes()
    }
    fn set_raw_attributes(&self, attributes: Map<String, Value>) -> Result<()> {
        self.attributes.set_raw_attributes(attributes)
    }
    fn plain_secret(&self) -> Result<Option<String>> {
        Ok(self.plain_secret.read().map_err(|_| state_error())?.clone())
    }
    fn set_secret<'a>(
        &'a self,
        hashing: &'a HashManager,
        secret: Option<&'a str>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let default_setter = ClientSecretSetter {
                client: self,
                hashing,
            };
            self.attributes
                .set_attribute_with_default_setter(
                    &self.metadata,
                    "secret",
                    secret.map_or(Value::Null, |value| Value::String(value.to_owned())),
                    &default_setter,
                )
                .await
        })
    }
}
// 调用级借用fallback，不注册为永久mutator，也不预先改变custom setter的输入。
struct ClientSecretSetter<'a> {
    client: &'a Client,
    hashing: &'a HashManager,
}
impl AttributeSetter for ClientSecretSetter<'_> {
    fn set(
        &self,
        value: Value,
        _snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async move {
            let plain = optional_string(Some(&value))?;
            {
                let mut temporary = self
                    .client
                    .plain_secret
                    .write()
                    .map_err(|_| state_error())?;
                temporary.clone_from(&plain);
            }
            // 固定默认mutator显式hashed；失败保临时明文副作用，但不发布raw patch。
            let encoded = cast_attribute_as_hashed_string(self.hashing, plain.as_deref()).await?;
            Ok(Map::from_iter([(
                "secret".to_owned(),
                encoded.map_or(Value::Null, Value::String),
            )]))
        })
    }
}

impl Serialize for Client {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.arrayable_attributes()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

/// 同一个可替换模型工厂决定仓储的表、主键、连接、水合及 ID 策略。
pub trait ClientModelFactory: Send + Sync {
    /// 返回唯一模型元数据。
    fn metadata(&self) -> &ModelMetadata;
    /// 将OAuth字符串参数还原为所选模型声明的主键类型；不按字符串外形猜UUID。
    ///
    /// 整数键仅接受可精确表示的i64；其它自定义策略可覆写此入口。
    fn identifier_from_protocol(&self, identifier: &str) -> Option<Value> {
        match self.metadata().key_type.as_str() {
            "int" | "integer" => identifier.parse::<i64>().ok().map(Value::from),
            _ => Some(Value::String(identifier.to_owned())),
        }
    }
    /// 默认接受带标准连字符的 UUID；自定义模型可使用其他原始键类型。
    fn is_valid_identifier(&self, value: &Value) -> bool {
        value.as_str().is_some_and(|text| {
            text.len() == 36
                && [8, 13, 18, 23]
                    .into_iter()
                    .all(|i| text.as_bytes()[i] == b'-')
                && uuid::Uuid::parse_str(text).is_ok()
        })
    }
    /// 按本模型的实际列策略生成绑定；自定义模型默认保留 text/i64，不根据外形猜 UUID。
    fn identifier_binding(
        &self,
        value: &Value,
        _backend: openrust::database::sea_orm::DbBackend,
    ) -> Result<openrust::database::sea_orm::Value> {
        match value {
            Value::String(value) => Ok(value.clone().into()),
            Value::Number(value) => value.as_i64().map(Into::into).ok_or_else(invalid_attribute),
            _ => Err(invalid_attribute()),
        }
    }
    /// 默认生成 UUIDv7；不预先字符串化应用自定义键。
    fn new_unique_id(&self) -> Result<Value> {
        Ok(Value::String(uuid::Uuid::now_v7().to_string()))
    }
    /// 从数据库原始快照创建所选模型，不调用 secret setter。
    fn new_from_builder(&self, attributes: Map<String, Value>) -> Result<Arc<dyn ClientModel>>;
}

/// 默认 UUID Passport 客户端工厂。
#[derive(Clone)]
pub struct ClientFactory {
    metadata: Arc<ModelMetadata>,
}
impl ClientFactory {
    /// 选择显式模型元数据；所有仓储操作应复用此实例。
    pub fn new(metadata: Arc<ModelMetadata>) -> Self {
        Self {
            metadata: with_client_casts(metadata),
        }
    }
}
impl Default for ClientFactory {
    fn default() -> Self {
        let mut metadata = ModelMetadata::new("oauth_clients", "openrust_passport::Client");
        metadata.key_type = "string".into();
        metadata.incrementing = false;
        metadata.hidden = vec!["secret".into()];
        Self::new(Arc::new(metadata))
    }
}
impl ClientModelFactory for ClientFactory {
    fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }
    fn identifier_binding(
        &self,
        value: &Value,
        backend: openrust::database::sea_orm::DbBackend,
    ) -> Result<openrust::database::sea_orm::Value> {
        let text = value.as_str().ok_or_else(invalid_attribute)?;
        if backend == openrust::database::sea_orm::DbBackend::Postgres {
            let identifier = uuid::Uuid::parse_str(text).map_err(|_| invalid_attribute())?;
            Ok(openrust::database::sea_orm::Value::Uuid(Some(identifier)))
        } else {
            Ok(text.to_owned().into())
        }
    }
    fn new_from_builder(&self, attributes: Map<String, Value>) -> Result<Arc<dyn ClientModel>> {
        Ok(Arc::new(Client::from_attributes(
            self.metadata.clone(),
            attributes,
        )))
    }
}

// 同一默认模型定义应用固定 casts；已完整声明的 Arc 不复制元数据。
fn with_client_casts(mut metadata: Arc<ModelMetadata>) -> Arc<ModelMetadata> {
    for name in [
        "grant_types",
        "scopes",
        "redirect_uris",
        "personal_access_client",
        "password_client",
        "revoked",
    ] {
        if !metadata.casts.contains_key(name)
            && let Some(cast) = client_cast(name)
        {
            Arc::make_mut(&mut metadata).casts.insert(name.into(), cast);
        }
    }
    metadata
}
fn client_cast(name: &str) -> Option<AttributeCast> {
    match name {
        "grant_types" | "scopes" | "redirect_uris" => Some(AttributeCast::Array),
        "personal_access_client" | "password_client" | "revoked" => Some(AttributeCast::Boolean),
        _ => None,
    }
}
fn state_error() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "passport_client_state_unavailable",
        "客户端模型状态不可用",
    )
}
fn invalid_attribute() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "passport_client_attribute_invalid",
        "客户端模型属性类型无效",
    )
}
fn optional_string(value: Option<&Value>) -> Result<Option<String>> {
    if value.is_some_and(|value| value.is_array() || value.is_object()) {
        return Err(invalid_attribute());
    }
    match AttributeCast::String.read(value.cloned())? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(invalid_attribute()),
    }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::String(value)) => !value.is_empty() && value != "0",
        Some(Value::Number(value)) => value.as_f64() != Some(0.0),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}
fn raw_array(value: &Value) -> Result<PhpArray> {
    let value = if value.is_string() {
        AttributeCast::Array
            .read(Some(value.clone()))?
            .ok_or_else(invalid_attribute)?
    } else {
        value.clone()
    };
    PhpArray::try_from(value)
}
// 默认访问器借用本模型；共同存储负责应用 getter 的优先级及锁外调用。
struct ClientGetter<'a> {
    client: &'a Client,
    name: &'a str,
}
impl AttributeGetter for ClientGetter<'_> {
    fn get(&self, raw: Option<&Value>, attributes: &Map<String, Value>) -> Result<Option<Value>> {
        match self.name {
            "redirect_uris" => redirect_attribute(raw, attributes),
            "grant_types" => grant_attribute(self.client, raw),
            "plain_secret" => self
                .client
                .plain_secret()
                .map(|value| Some(value.map_or(Value::Null, Value::String))),
            _ => Err(invalid_attribute()),
        }
    }
}
fn redirect_attribute(
    raw: Option<&Value>,
    attributes: &Map<String, Value>,
) -> Result<Option<Value>> {
    // typed空数组/对象代表已解码的非empty JSON文本，不能按PHP空值恢复legacy名单。
    let redirects = if let Some(value) = raw
        .filter(|value| matches!(value, Value::Array(_) | Value::Object(_)) || truthy(Some(value)))
    {
        raw_array(value)?
    } else if truthy(attributes.get("redirect")) {
        optional_string(attributes.get("redirect"))?
            .ok_or_else(invalid_attribute)?
            .split(',')
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .into()
    } else {
        PhpArray::default()
    };
    Ok(Some(redirects.into()))
}

fn grant_attribute<M: ClientModel + ?Sized>(
    model: &M,
    raw: Option<&Value>,
) -> Result<Option<Value>> {
    if let Some(value) = raw.filter(|value| !value.is_null()) {
        return serde_json::to_value(raw_array(value)?)
            .map(Some)
            .map_err(Error::internal);
    }
    let grants = [
        ("authorization_code", !model.redirect_uris()?.is_empty()),
        (
            "client_credentials",
            model.confidential()? && model.first_party()?,
        ),
        ("implicit", !model.redirect_uris()?.is_empty()),
        (
            "password",
            truthy(model.get_attribute("password_client")?.as_ref()),
        ),
        (
            "personal_access",
            truthy(model.get_attribute("personal_access_client")?.as_ref())
                && model.confidential()?,
        ),
        ("refresh_token", true),
        ("urn:ietf:params:oauth:grant-type:device_code", true),
    ]
    .into_iter()
    .filter(|(_, enabled)| *enabled)
    .map(|(name, _)| name)
    .collect::<Vec<_>>();
    serde_json::to_value(grants)
        .map(Some)
        .map_err(Error::internal)
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

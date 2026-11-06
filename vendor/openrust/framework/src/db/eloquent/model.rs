//! 模型元数据与多态持久身份；登录标识不承担模型主键职责。

use crate::{
    Result,
    db::{Databases, Db},
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

use super::AttributeCast;

/// Rust 模型显式声明的数据库元数据；不从 Guard/Provider 名推导类型或主键。
///
/// 表名和模型类标识须由应用声明，因为 Rust 不提供 PHP 的运行期类模型反射。
/// 默认主键、类型、递增及可见性对应 Laravel Model；不自动执行数据库 I/O。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelMetadata {
    /// 模型逻辑表名；最终物理表名仍由同一连接解析器处理。
    pub table: String,
    /// 稳定模型类标识；应用可在多态映射扩展中改为已登记别名。
    pub class_name: String,
    /// 模型主键名称，默认 id，独立于登录标识字段。
    pub key_name: String,
    /// 模型主键转换声明，默认 int；具体转换由模型属性机制负责。
    pub key_type: String,
    /// 是否由数据库递增生成主键，默认 true。
    pub incrementing: bool,
    /// 显式模型连接；None 才继承领域连接设置。
    pub connection: Option<String>,
    /// 序列化排除字段，默认空。
    pub hidden: Vec<String>,
    /// 非空时只序列化这些字段，默认空表示不额外筛选。
    pub visible: Vec<String>,
    /// 显式属性转换，主键声明可覆盖隐式的递增主键转换。
    pub casts: BTreeMap<String, AttributeCast>,
}
impl ModelMetadata {
    /// 创建显式表名和类标识的模型定义；其余采用官方基础默认。
    pub fn new(table: impl Into<String>, class_name: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            class_name: class_name.into(),
            key_name: "id".into(),
            key_type: "int".into(),
            incrementing: true,
            connection: None,
            hidden: Vec::new(),
            visible: Vec::new(),
            casts: BTreeMap::new(),
        }
    }
    /// 返回读取/赋值使用的转换；只有递增主键才隐式注入 `key_type`。
    pub fn effective_cast(&self, name: &str) -> Result<Option<AttributeCast>> {
        if let Some(cast) = self.casts.get(name) {
            return Ok(Some(*cast));
        }
        if name != self.key_name || !self.incrementing {
            return Ok(None);
        }
        let cast = match self.key_type.as_str() {
            "int" | "integer" => AttributeCast::Integer,
            "string" => AttributeCast::String,
            "bool" | "boolean" => AttributeCast::Boolean,
            "float" | "double" | "real" => AttributeCast::Float,
            "array" | "json" => AttributeCast::Array,
            _ => {
                return Err(crate::Error::custom_with_status(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "model_attribute_cast_unsupported",
                    "模型主键声明的转换尚未实现",
                ));
            }
        };
        Ok(Some(cast))
    }
    /// 按模型连接、领域fallback、应用默认连接的顺序选择同一个公开 Db。
    ///
    /// 显式空名或字符串0沿数据库管理器的PHP假值语义选应用默认，不回退到领域连接。
    pub fn resolve_connection(&self, databases: &Databases, fallback: Option<&str>) -> Result<Db> {
        match self.connection.as_deref().or(fallback) {
            Some(name) if !name.is_empty() && name != "0" => databases.get(name),
            _ => Ok(databases.default_database()),
        }
    }
    /// 对应 `getArrayableAttributes` 的可见性过滤；不执行 cast、追加属性或关系加载。
    pub fn arrayable_attributes(&self, attributes: &Map<String, Value>) -> Map<String, Value> {
        attributes
            .iter()
            .filter(|(key, _)| {
                (self.visible.is_empty() || self.visible.contains(key))
                    && !self.hidden.contains(key)
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
}

/// 已加载模型提供的持久身份能力；允许登录标识与模型主键不同。
///
/// 返回模型自身 `getKey` 的转换后原始值，未持久化模型可为 null；
/// 不调用 `integer_id`，不把复合键或浮点数偷偷字符串化。
pub trait ModelIdentity: Send + Sync {
    /// 返回所选模型定义。
    fn model_metadata(&self) -> &ModelMetadata;
    /// 返回模型主键值，保留字符串、数字和 null 的类型。
    fn model_key(&self) -> Result<Value>;
    /// 返回模型多态类或应用显式映射的别名；不使用认证 Provider 名。
    fn morph_class(&self) -> Result<Value> {
        Ok(Value::String(self.model_metadata().class_name.clone()))
    }
}

/// 从模型自身取得的多态外键快照；不包含登录密码或账户其他属性。
///
/// 不实现 Debug/Serialize，写入关系时由调用方显式取两个原始值。
#[derive(Clone, PartialEq)]
pub struct MorphIdentity {
    model_type: Value,
    key: Value,
}
impl MorphIdentity {
    /// 从已选模型能力提取身份；不是从登录标识或 Guard 名构造。
    pub fn from_model(model: &dyn ModelIdentity) -> Result<Self> {
        Ok(Self {
            model_type: model.morph_class()?,
            key: model.model_key()?,
        })
    }
    /// 多态类型值；保留显式别名的原类型。
    pub fn model_type(&self) -> &Value {
        &self.model_type
    }
    /// 模型主键原值；与认证标识无隐式等价关系。
    pub fn key(&self) -> &Value {
        &self.key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn primary_key_cast_is_injected_only_for_incrementing_and_explicit_cast_wins() -> Result<()> {
        let mut metadata = ModelMetadata::new("accounts", "Account");
        assert_eq!(metadata.effective_cast("id")?, Some(AttributeCast::Integer));
        assert_eq!(metadata.effective_cast("name")?, None);
        metadata.key_type = "string".into();
        assert_eq!(metadata.effective_cast("id")?, Some(AttributeCast::String));
        metadata.incrementing = false;
        assert_eq!(metadata.effective_cast("id")?, None);
        metadata.casts.insert("id".into(), AttributeCast::Integer);
        assert_eq!(metadata.effective_cast("id")?, Some(AttributeCast::Integer));
        metadata.incrementing = true;
        metadata.casts.insert("id".into(), AttributeCast::String);
        metadata.key_type = "not-implemented".into();
        assert_eq!(metadata.effective_cast("id")?, Some(AttributeCast::String));
        metadata.casts.clear();
        assert!(
            matches!(metadata.effective_cast("id"), Err(error) if error.code()=="model_attribute_cast_unsupported")
        );
        Ok(())
    }
    #[test]
    fn selected_model_connection_precedes_fallback_and_false_names_use_default() -> Result<()> {
        // 用公开表名解析结果区分连接，无需启用 auth 的私有句柄身份辅助方法。
        let database = |prefix| -> Result<Db> {
            Ok(Db::with_table_names(crate::db::DatabaseTableNames::new(
                prefix,
                BTreeMap::new(),
            )?))
        };
        let default = database("default_")?;
        let domain = database("domain_")?;
        let custom = database("custom_")?;
        let databases = Databases::new(
            default,
            BTreeMap::from([("domain".into(), domain), ("custom".into(), custom)]),
        );
        let mut metadata = ModelMetadata::new("custom_clients", "AppClient");
        assert_eq!(
            metadata
                .resolve_connection(&databases, Some("domain"))?
                .table_names()
                .resolve(&metadata.table)?,
            "domain_custom_clients"
        );
        metadata.connection = Some("custom".into());
        assert_eq!(
            metadata
                .resolve_connection(&databases, Some("domain"))?
                .table_names()
                .resolve(&metadata.table)?,
            "custom_custom_clients"
        );
        for name in ["", "0"] {
            metadata.connection = Some(name.into());
            assert_eq!(
                metadata
                    .resolve_connection(&databases, Some("domain"))?
                    .table_names()
                    .resolve(&metadata.table)?,
                "default_custom_clients"
            );
        }
        metadata.connection = Some("missing".into());
        assert!(metadata.resolve_connection(&databases, None).is_err());
        Ok(())
    }
    #[test]
    fn visibility_intersection_does_not_expose_hidden_even_when_whitelisted() {
        let mut metadata = ModelMetadata::new("clients", "Client");
        metadata.hidden = vec!["secret".into()];
        metadata.visible = vec!["id".into(), "secret".into()];
        let raw = Map::from_iter([
            ("id".into(), Value::from(4)),
            ("secret".into(), Value::from("not-public")),
            ("name".into(), Value::from("client")),
        ]);
        assert_eq!(
            metadata.arrayable_attributes(&raw),
            Map::from_iter([("id".into(), Value::from(4))])
        );
        assert_eq!(raw.len(), 3);
    }
    #[test]
    fn identity_keeps_raw_model_key_and_numeric_morph_alias() -> Result<()> {
        struct Account {
            metadata: ModelMetadata,
            key: Value,
        }
        impl ModelIdentity for Account {
            fn model_metadata(&self) -> &ModelMetadata {
                &self.metadata
            }
            fn model_key(&self) -> Result<Value> {
                Ok(self.key.clone())
            }
            fn morph_class(&self) -> Result<Value> {
                Ok(0.into())
            }
        }
        for key in [
            Value::from(42),
            Value::from("0042"),
            Value::from("uuid-model-key"),
            Value::Null,
        ] {
            let account = Account {
                metadata: ModelMetadata::new("members", "AppMember"),
                key: key.clone(),
            };
            let identity = MorphIdentity::from_model(&account)?;
            assert_eq!(identity.key(), &key);
            assert_eq!(identity.model_type(), &Value::from(0));
        }
        Ok(())
    }
}

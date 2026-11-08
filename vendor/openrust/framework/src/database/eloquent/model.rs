//! 模型元数据与多态持久身份；登录标识不承担模型主键职责。

use crate::{
    Result,
    database::{Databases, Db},
};
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::collections::HashSet;

use super::{AttributeCast, AttributeValue, ModelDateContext};

/// 模型已加载关系的输出名称规则。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RelationNaming {
    /// Laravel 默认的蛇形名称。
    #[default]
    Snake,
    /// 保留加载时的名称。
    Original,
}

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
    /// 默认将已加载关系的输出名称转换为蛇形命名。
    pub relation_naming: RelationNaming,
    /// 显式属性转换，主键声明可覆盖隐式的递增主键转换。
    pub casts: IndexMap<String, AttributeCast>,
    /// 是否在模型持久化流程中自动维护时间戳。
    pub timestamps: bool,
    /// 创建时间列；None 关闭该列。
    pub created_at_column: Option<String>,
    /// 更新时间列；None 关闭该列。
    pub updated_at_column: Option<String>,
    /// 覆盖持久化时间使用的 PHP 日期格式。
    pub date_format: Option<String>,
    /// 模型额外声明为日期的原始字段。
    pub date_attributes: Vec<String>,
    /// 持久化前是否调用唯一 ID 生成。
    pub uses_unique_ids: bool,
    /// 按声明顺序处理的唯一 ID 字段。
    pub unique_id_columns: Vec<String>,
    fillable: Vec<String>,
    guarded: Vec<String>,
    appends: Vec<String>,
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
            relation_naming: RelationNaming::Snake,
            casts: IndexMap::new(),
            timestamps: true,
            created_at_column: Some("created_at".into()),
            updated_at_column: Some("updated_at".into()),
            date_format: None,
            date_attributes: Vec::new(),
            uses_unique_ids: false,
            unique_id_columns: Vec::new(),
            fillable: Vec::new(),
            guarded: vec!["*".into()],
            appends: Vec::new(),
        }
    }

    /// 返回模型数组中需要追加的访问器名称。
    pub fn get_appends(&self) -> &[String] {
        &self.appends
    }

    /// 替换追加字段声明，保留传入的顺序。
    pub fn set_appends(&mut self, appends: Vec<String>) -> &mut Self {
        self.appends = appends;
        self
    }

    /// 按首次出现顺序合并追加字段。
    pub fn merge_appends(&mut self, appends: Vec<String>) -> &mut Self {
        if !appends.is_empty() {
            merge_assignment_fields(&mut self.appends, appends);
        }
        self
    }

    /// 追加一个访问器名称。
    pub fn append(&mut self, name: impl Into<String>) -> &mut Self {
        self.merge_appends(vec![name.into()])
    }

    /// 判断模型是否声明了此追加字段。
    pub fn has_appended(&self, name: &str) -> bool {
        self.appends
            .iter()
            .any(|key| crate::support::php_loose_string_equal(key, name))
    }

    /// 清除所有追加字段。
    pub fn without_appends(&mut self) -> &mut Self {
        self.appends.clear();
        self
    }
    /// 返回模型声明的批量赋值白名单。
    pub fn get_fillable(&self) -> &[String] {
        &self.fillable
    }

    /// 替换批量赋值白名单，保留原声明顺序及重复项。
    pub fn fillable(&mut self, fields: Vec<String>) -> &mut Self {
        self.fillable = fields;
        self
    }

    /// 按首次出现顺序合并白名单；空输入保持现有声明。
    pub fn merge_fillable(&mut self, fields: Vec<String>) -> &mut Self {
        if !fields.is_empty() {
            merge_assignment_fields(&mut self.fillable, fields);
        }
        self
    }

    /// 返回模型声明的批量赋值拒绝列表。
    pub fn get_guarded(&self) -> &[String] {
        &self.guarded
    }

    /// 替换拒绝列表；空列表关闭此模型的拒绝列表。
    pub fn guard(&mut self, fields: Vec<String>) -> &mut Self {
        self.guarded = fields;
        self
    }

    /// 按首次出现顺序合并拒绝列表，空输入也会去重。
    pub fn merge_guarded(&mut self, fields: Vec<String>) -> &mut Self {
        merge_assignment_fields(&mut self.guarded, fields);
        self
    }

    /// 返回模型声明是否默认保护全部字段。
    pub fn totally_guarded(&self) -> bool {
        self.fillable.is_empty() && self.guarded == ["*"]
    }

    pub(super) fn cast_names(&self) -> impl Iterator<Item = &str> {
        self.incrementing
            .then_some(self.key_name.as_str())
            .into_iter()
            .chain(
                self.casts
                    .keys()
                    .map(String::as_str)
                    .filter(|name| !self.incrementing || *name != self.key_name),
            )
    }

    /// 返回读取/赋值使用的转换；只有递增主键才隐式注入 `key_type`。
    pub fn effective_cast(&self, name: &str) -> Result<Option<AttributeCast>> {
        if let Some(cast) = self.casts.get(name) {
            return Ok(Some(cast.clone()));
        }
        if name != self.key_name || !self.incrementing {
            return Ok(None);
        }
        let key_type = self
            .key_type
            .trim_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
            .to_ascii_lowercase();
        let cast = match key_type.as_str() {
            "int" | "integer" => AttributeCast::Integer,
            "string" => AttributeCast::String,
            "bool" | "boolean" => AttributeCast::Boolean,
            "float" | "double" | "real" => AttributeCast::Float,
            "array" | "json" => AttributeCast::Array,
            "hashed" => AttributeCast::Hashed,
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
    /// 对独立原始值执行本字段已支持的读取转换及原始声明检查。
    ///
    /// 复用模型属性的同一 cast 管线，不创建属性存储，不运行 getter、setter 或 SQL。
    /// 这不是完整 `getAttribute`：调用者负责访问器、字段读取和缺项策略。
    /// primitive null 保留读取短路；非空非规范 `Hashed` 声明仍返回原有转换错误。
    pub fn cast_attribute(&self, name: &str, raw: Option<Value>) -> Result<Option<Value>> {
        self.read_cast_attribute(name, raw)
    }
    // 归一类型供读取switch使用；落入class检查的入口仍须验证原始声明。
    pub(crate) fn read_cast_attribute(
        &self,
        name: &str,
        raw: Option<Value>,
    ) -> Result<Option<Value>> {
        self.cast_attribute_value(
            name,
            raw.map(AttributeValue::from),
            &ModelDateContext::default(),
        )?
        .map(AttributeValue::try_into_json)
        .transpose()
    }

    /// 对独立属性执行模型 cast，保留日期对象和缺省值类别。
    pub fn cast_attribute_value(
        &self,
        name: &str,
        raw: Option<AttributeValue>,
        dates: &ModelDateContext,
    ) -> Result<Option<AttributeValue>> {
        let snapshot = raw
            .iter()
            .map(|value| (name.to_owned(), value.clone()))
            .collect();
        self.cast_value_with_snapshot(name, raw, &snapshot, dates)
    }

    pub(super) fn cast_value_with_snapshot(
        &self,
        name: &str,
        raw: Option<AttributeValue>,
        snapshot: &super::AttributeMap,
        dates: &ModelDateContext,
    ) -> Result<Option<AttributeValue>> {
        let cast = self.effective_cast(name)?;
        if let Some(AttributeCast::Class(caster)) = &cast {
            return caster
                .resolve()?
                .get(name, raw.as_ref(), snapshot)
                .map(AttributeValue::from)
                .map(Some);
        }
        let Some(raw) = raw else { return Ok(None) };
        let Some(cast) = cast else {
            return if !raw.is_null() && self.is_implicit_date(name) {
                dates.read(self, &AttributeCast::DateTime, raw).map(Some)
            } else {
                Ok(Some(raw))
            };
        };
        if cast.is_date_read() {
            return dates.read(self, &cast, raw).map(Some);
        }
        if cast == AttributeCast::Hashed && !raw.is_null() {
            self.check_raw_class_cast(name)?;
        }
        super::attribute_cast::cast_read_value(&cast, Some(raw.try_into_json()?))
            .map(|value| value.map(AttributeValue::from))
    }

    /// 显式将属性读值投影为 JSON，日期使用默认 UTC 或 cast 的自定义格式。
    pub fn project_attribute_json(&self, name: &str, value: AttributeValue) -> Result<Value> {
        self.project_date_value(name, value, &ModelDateContext::default(), false)
    }

    pub(super) fn project_date_value(
        &self,
        name: &str,
        value: AttributeValue,
        dates: &ModelDateContext,
        from_accessor: bool,
    ) -> Result<Value> {
        let AttributeValue::Date(date) = value else {
            return value.try_into_json();
        };
        let date = date.snapshot()?;
        if from_accessor {
            return dates.serialize(&date);
        }
        match self.effective_cast(name)? {
            Some(
                AttributeCast::CustomDateTime(format)
                | AttributeCast::ImmutableCustomDateTime(format),
            ) => date.format(&format).map(Value::String),
            _ => dates.serialize(&date),
        }
    }

    pub(super) fn is_implicit_date(&self, name: &str) -> bool {
        self.date_attribute_names().any(|field| field == name)
    }

    pub(super) fn date_attribute_names(&self) -> impl Iterator<Item = &str> {
        [
            self.created_at_column.as_deref(),
            self.updated_at_column.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter(|_| self.uses_timestamps())
        .chain(self.date_attributes.iter().map(String::as_str))
    }

    // setAttribute在JSON/hashed分派及null旁路之前检查原始class声明。
    pub(crate) fn write_cast(&self, name: &str) -> Result<Option<AttributeCast>> {
        self.check_raw_class_cast(name)?;
        self.effective_cast(name)
    }
    pub(super) fn check_raw_class_cast(&self, name: &str) -> Result<()> {
        // 显式enum声明自身即规范类型，不继承被覆盖的key_type原文。
        if self.casts.contains_key(name) || name != self.key_name || !self.incrementing {
            return Ok(());
        }
        let raw = self.key_type.split(':').next().unwrap_or(&self.key_type);
        if matches!(
            raw,
            "int"
                | "integer"
                | "string"
                | "bool"
                | "boolean"
                | "float"
                | "double"
                | "real"
                | "array"
                | "json"
                | "hashed"
        ) {
            return Ok(());
        }
        Err(crate::Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "model_attribute_cast_unsupported",
            "模型属性声明的转换类型无效或尚未实现",
        ))
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
            .filter(|(key, _)| self.attribute_is_arrayable(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    pub(super) fn attribute_is_arrayable(&self, name: &str) -> bool {
        (self.visible.is_empty() || self.visible.iter().any(|field| field == name))
            && !self.hidden.iter().any(|field| field == name)
    }
}

fn merge_assignment_fields(existing: &mut Vec<String>, fields: Vec<String>) {
    existing.extend(fields);
    let mut seen = HashSet::with_capacity(existing.len());
    existing.retain(|field| seen.insert(field.clone()));
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
    fn assignment_declarations_preserve_defaults_and_merge_order() {
        let mut metadata = ModelMetadata::new("records", "example::Record");
        assert_eq!(metadata.get_fillable(), &[] as &[String]);
        assert_eq!(metadata.get_guarded(), &["*"]);
        assert!(metadata.totally_guarded());

        metadata
            .fillable(vec!["name".into(), "name".into(), "Name".into()])
            .merge_fillable(Vec::new());
        assert_eq!(metadata.get_fillable(), &["name", "name", "Name"]);
        assert!(!metadata.totally_guarded());
        metadata.merge_fillable(vec!["priority".into(), "Name".into()]);
        assert_eq!(metadata.get_fillable(), &["name", "Name", "priority"]);

        metadata
            .fillable(Vec::new())
            .guard(vec!["*".into(), "*".into()]);
        assert!(!metadata.totally_guarded());
        metadata.merge_guarded(Vec::new());
        assert!(metadata.totally_guarded());
        metadata.merge_guarded(vec!["owner_id".into(), "*".into(), "owner_id".into()]);
        assert_eq!(metadata.get_guarded(), &["*", "owner_id"]);
        assert!(!metadata.totally_guarded());
        metadata.guard(Vec::new());
        assert_eq!(metadata.get_guarded(), &[] as &[String]);
    }

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
            Ok(Db::with_table_names(
                crate::database::DatabaseTableNames::new(prefix, BTreeMap::new())?,
            ))
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

#[cfg(test)]
mod key_cast_state_tests {
    use super::*;

    #[test]
    fn key_cast_uses_php_trim_ascii_case_and_existing_supported_types() -> Result<()> {
        let mut metadata = ModelMetadata::new("records", "app::Record");
        for (declaration, expected) in [
            (" INT ", AttributeCast::Integer),
            ("\0\t\n\r\u{b}HaShEd\0", AttributeCast::Hashed),
            (" JSON ", AttributeCast::Array),
        ] {
            metadata.key_type = declaration.into();
            assert_eq!(metadata.effective_cast("id")?, Some(expected));
        }
        for declaration in [
            "\u{c}int",
            "int\u{c}",
            "\u{a0}int",
            "int\u{2003}",
            "json:unicode",
            "unsupported",
        ] {
            metadata.key_type = declaration.into();
            assert!(
                matches!(metadata.effective_cast("id"), Err(error) if error.code() == "model_attribute_cast_unsupported")
            );
        }
        metadata.casts.insert("id".into(), AttributeCast::String);
        assert_eq!(metadata.effective_cast("id")?, Some(AttributeCast::String));
        metadata.casts.clear();
        metadata.incrementing = false;
        assert_eq!(metadata.effective_cast("id")?, None);
        Ok(())
    }
}

#[cfg(test)]
mod public_cast_extension_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn public_cast_extension_uses_the_same_read_declaration_and_null_rules() -> Result<()> {
        let mut metadata = ModelMetadata::new("records", "app::Record");
        metadata.key_type = "hashed".into();
        assert_eq!(
            metadata.cast_attribute("id", Some(json!("plain")))?,
            Some(json!("plain"))
        );
        metadata.key_type = " HaShEd ".into();
        assert!(
            matches!(metadata.cast_attribute("id", Some(json!(1))), Err(error) if error.code() == "model_attribute_cast_unsupported" && error.status().as_u16() == 500)
        );
        assert_eq!(
            metadata.cast_attribute("id", Some(Value::Null))?,
            Some(Value::Null)
        );
        assert_eq!(metadata.cast_attribute("id", None)?, None);
        metadata.key_type = " INT ".into();
        assert_eq!(
            metadata.cast_attribute("id", Some(json!("01")))?,
            Some(json!(1))
        );
        metadata.key_type = "unsupported".into();
        assert!(
            matches!(metadata.cast_attribute("id", Some(Value::Null)), Err(error) if error.code() == "model_attribute_cast_unsupported")
        );
        metadata.casts.insert("id".into(), AttributeCast::Hashed);
        assert_eq!(
            metadata.cast_attribute("id", Some(json!("plain")))?,
            Some(json!("plain"))
        );
        assert_eq!(
            metadata.cast_attribute("unconfigured", Some(json!({})))?,
            Some(json!({}))
        );
        Ok(())
    }
}

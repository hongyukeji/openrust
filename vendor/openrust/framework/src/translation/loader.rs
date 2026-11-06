//! 统一翻译来源的快照契约；对应 Laravel Loader，异步阶段先完整物化再同步读取。

use super::{Locale, MessageCatalog};
use crate::{Error, Result, support::BoxFuture};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

/// 同一语言候选中的一个资源；星号group表示JSON全局消息。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TranslationResource {
    /// 规范语言标签，不在来源内应用fallback。
    pub locale: Locale,
    /// 资源组名称，`*`表示全局JSON键。
    pub group: String,
    /// 可选模块命名空间；None和空字符串不是同一个合法声明。
    pub namespace: Option<String>,
}
/// 消息键到Catalog可接受的字符串、plural或select结构；空字符串是有效消息。
pub type TranslationLines = BTreeMap<String, Value>;

/// 已验证不可变资源快照；同步load不得再查数据库或文件。
pub trait TranslationLoadSession: std::any::Any + Send + Sync {
    /// 返回本快照实际拥有的资源，不包含语言回退生成的虚拟资源。
    fn resources(&self) -> &[TranslationResource];
    /// 读取精确语言/group/namespace；缺失返回空Map，不执行fallback。
    fn load(
        &self,
        locale: &Locale,
        group: &str,
        namespace: Option<&str>,
    ) -> Result<TranslationLines>;
}
/// 可替换翻译Loader；一次begin物化一个可原子发布的消息版本。
pub trait TranslationLoader: Send + Sync {
    /// 完整读取并验证来源；失败不得发布半份快照。
    fn begin_snapshot(&self) -> BoxFuture<'_, Result<Arc<dyn TranslationLoadSession>>>;
    /// 按该消费者最后成功发布的快照判断更新；独立捕获不能推进其他消费者游标。
    ///
    /// 来源可用Any识别自己的不可变session；默认保持既有自定义来源的检查行为。
    fn needs_refresh_since<'a>(
        &'a self,
        _published: &'a dyn TranslationLoadSession,
    ) -> BoxFuture<'a, Result<bool>> {
        self.needs_refresh()
    }
    /// 在显式异步边界判断是否需刷新；普通文件默认不自动轮询。
    fn needs_refresh(&self) -> BoxFuture<'_, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}

pub(crate) struct MaterializedSession {
    resources: Vec<TranslationResource>,
    lines: BTreeMap<TranslationResource, TranslationLines>,
}
impl MaterializedSession {
    pub(crate) fn new(lines: BTreeMap<TranslationResource, TranslationLines>) -> Result<Self> {
        for (resource, messages) in &lines {
            validate_lines(resource, messages)?;
        }
        Ok(Self {
            resources: lines.keys().cloned().collect(),
            lines,
        })
    }
}
impl TranslationLoadSession for MaterializedSession {
    fn resources(&self) -> &[TranslationResource] {
        &self.resources
    }
    fn load(
        &self,
        locale: &Locale,
        group: &str,
        namespace: Option<&str>,
    ) -> Result<TranslationLines> {
        Ok(self
            .lines
            .get(&TranslationResource {
                locale: locale.clone(),
                group: group.into(),
                namespace: namespace.map(str::to_owned),
            })
            .cloned()
            .unwrap_or_default())
    }
}

pub(crate) fn validate_lines(
    resource: &TranslationResource,
    lines: &TranslationLines,
) -> Result<()> {
    if resource.locale.as_str().len() > 128
        || resource.group.is_empty()
        || resource.group.len() > 255
        || resource.group.chars().any(char::is_control)
        || resource.namespace.as_ref().is_some_and(|name| {
            name.is_empty() || name.len() > 255 || name.chars().any(char::is_control)
        })
        || lines
            .keys()
            .any(|key| key.is_empty() || key.len() > 512 || key.chars().any(char::is_control))
    {
        return Err(invalid_resource());
    }
    let serialized = serde_json::to_string(lines).map_err(|_| invalid_resource())?;
    MessageCatalog::new()
        .merge_json(resource.locale.as_str(), &serialized)
        .map_err(|_| invalid_resource())
}
pub(crate) fn invalid_resource() -> Error {
    Error::custom("i18n_translation_resource_invalid", "翻译资源结构无效")
}

pub(crate) fn parse_lines(bytes: &[u8]) -> Result<TranslationLines> {
    match parse_value(bytes)? {
        Value::Object(lines) => Ok(lines.into_iter().collect()),
        _ => Err(invalid_resource()),
    }
}
pub(crate) fn parse_value(bytes: &[u8]) -> Result<Value> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = <UniqueValue as serde::Deserialize>::deserialize(&mut deserializer)
        .map_err(|_| invalid_resource())?;
    deserializer.end().map_err(|_| invalid_resource())?;
    Ok(value.0)
}
struct UniqueValue(Value);
impl<'de> serde::Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor).map(Self)
    }
}
struct UniqueVisitor;
impl<'de> serde::de::Visitor<'de> for UniqueVisitor {
    type Value = Value;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("无重复键的JSON资源")
    }
    fn visit_bool<E: serde::de::Error>(self, value: bool) -> std::result::Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: serde::de::Error>(self, value: i64) -> std::result::Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_u64<E: serde::de::Error>(self, value: u64) -> std::result::Result<Value, E> {
        Ok(Value::from(value))
    }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> std::result::Result<Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("无效JSON数值"))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<Value, E> {
        Ok(Value::String(value.into()))
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> std::result::Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(
        self,
        mut sequence: A,
    ) -> std::result::Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueValue>()? {
            values.push(value.0);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut source: A,
    ) -> std::result::Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some((key, value)) = source.next_entry::<String, UniqueValue>()? {
            if values.insert(key, value.0).is_some() {
                return Err(serde::de::Error::custom("重复消息键"));
            }
        }
        Ok(Value::Object(values))
    }
}

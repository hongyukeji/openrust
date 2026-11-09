//! 按名称保存可扩展消息包，默认包承担视图错误代理。

use super::message_bag::{
    MessageBag, MessageBagContract, MessageJsonOptions, MessageKeys, MessageLookup, MessageValue,
};
use super::{ArrayKey, Collection};
use crate::Result;
use serde::{Serialize, Serializer};
use std::fmt;

/// 拥有契约对象的命名包句柄。
pub struct BagHandle(Box<dyn MessageBagContract>);
impl BagHandle {
    /// 从外部契约对象建立句柄。
    pub fn new(bag: Box<dyn MessageBagContract>) -> Self {
        Self(bag)
    }
    #[cfg(feature = "view")]
    pub(crate) fn into_bag(self) -> Box<dyn MessageBagContract> {
        self.0
    }
    /// 借用原契约实例。
    pub fn as_bag(&self) -> &dyn MessageBagContract {
        self.0.as_ref()
    }
    /// 可变借用原契约实例。
    pub fn as_bag_mut(&mut self) -> &mut dyn MessageBagContract {
        self.0.as_mut()
    }
}
impl Serialize for BagHandle {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.0.to_array().serialize(serializer)
    }
}
impl fmt::Debug for BagHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("BagHandle").finish_non_exhaustive()
    }
}

/// 已存包借用或缺失时的独立临时空包。
pub enum BagRead<'a> {
    /// 借用已存实例。
    Stored(&'a dyn MessageBagContract),
    /// 独立临时包，不插入容器。
    Missing(MessageBag),
}
impl BagRead<'_> {
    /// 读取契约。
    pub fn as_bag(&self) -> &dyn MessageBagContract {
        match self {
            Self::Stored(bag) => *bag,
            Self::Missing(bag) => bag,
        }
    }
}

/// 已存可变借用或缺失时的独立临时空包。
pub enum BagWrite<'a> {
    /// 修改已存实例。
    Stored(&'a mut dyn MessageBagContract),
    /// 修改临时包，不插入容器。
    Missing(MessageBag),
}
impl BagWrite<'_> {
    /// 可变读取契约。
    pub fn as_bag_mut(&mut self) -> &mut dyn MessageBagContract {
        match self {
            Self::Stored(bag) => *bag,
            Self::Missing(bag) => bag,
        }
    }
}

/// 命名错误包集合。
#[derive(Default)]
pub struct ViewErrorBag {
    bags: Collection<BagHandle>,
}
impl ViewErrorBag {
    #[cfg(feature = "view")]
    pub(crate) fn into_bags(self) -> Collection<BagHandle> {
        self.bags
    }
    /// 建立空命名包集合。
    pub fn new() -> Self {
        Self::default()
    }
    /// 直接键是否存在；不解析点路径。
    pub fn has_bag(&self, key: impl Into<ArrayKey>) -> bool {
        self.bags.get(key).is_some()
    }
    /// 读取包；直接键优先，缺失返回临时空包。
    pub fn get_bag(&self, key: impl Into<ArrayKey>) -> BagRead<'_> {
        match self.bags.get(key) {
            Some(bag) => BagRead::Stored(bag.as_bag()),
            None => BagRead::Missing(MessageBag::new()),
        }
    }
    /// 修改已存包或独立临时包。
    pub fn get_bag_mut(&mut self, key: impl Into<ArrayKey>) -> BagWrite<'_> {
        match self.bags.get_mut(key) {
            Some(bag) => BagWrite::Stored(bag.as_bag_mut()),
            None => BagWrite::Missing(MessageBag::new()),
        }
    }
    /// 借用全部命名包，保持原键和顺序。
    pub fn bags(&self) -> &Collection<BagHandle> {
        &self.bags
    }
    /// 添加或替换命名包。
    pub fn put(
        &mut self,
        key: impl Into<ArrayKey>,
        bag: Box<dyn MessageBagContract>,
    ) -> Result<()> {
        self.bags.try_put(key, BagHandle::new(bag))
    }
    /// 默认包的消息数。
    pub fn count(&self) -> usize {
        self.bags
            .get("default")
            .map_or(0, |bag| bag.as_bag().count())
    }
    /// 默认包是否含消息。
    pub fn any(&self) -> bool {
        self.count() > 0
    }
    /// 默认包的首条消息。
    pub fn default_first(&self, key: Option<&str>, format: Option<&str>) -> Result<Option<String>> {
        self.get_bag("default").as_bag().first(key, format)
    }
    /// 默认包的键查询。
    pub fn default_get(&self, key: &str, format: Option<&str>) -> Result<MessageLookup> {
        self.get_bag("default").as_bag().get(key, format)
    }
    /// 默认包的存在检查。
    pub fn default_has(&self, keys: MessageKeys) -> Result<bool> {
        self.get_bag("default").as_bag().has(keys)
    }
    /// 默认包任一键是否有消息。
    pub fn default_has_any(&self, keys: MessageKeys) -> Result<bool> {
        self.get_bag("default").as_bag().has_any(keys)
    }
    /// 默认包指定键是否全缺失。
    pub fn default_missing(&self, keys: MessageKeys) -> Result<bool> {
        self.get_bag("default").as_bag().missing(keys)
    }
    /// 默认包的键，缺失时为空。
    pub fn default_keys(&self) -> Vec<ArrayKey> {
        self.get_bag("default").as_bag().keys()
    }
    /// 默认包的全部消息。
    pub fn default_all(&self, format: Option<&str>) -> Result<Collection<MessageValue>> {
        self.get_bag("default").as_bag().all(format)
    }
    /// 默认包去重后的全部消息。
    pub fn default_unique(&self, format: Option<&str>) -> Result<Collection<MessageValue>> {
        self.get_bag("default").as_bag().unique(format)
    }
    /// 默认包当前格式。
    pub fn default_format(&self) -> String {
        self.get_bag("default").as_bag().format().to_owned()
    }
    /// 默认包原始消息，返回独立副本。
    pub fn default_messages(&self) -> Collection<Collection<MessageValue>> {
        self.get_bag("default").as_bag().messages().clone()
    }
    /// `default_messages` 别名。
    pub fn default_get_messages(&self) -> Collection<Collection<MessageValue>> {
        self.default_messages()
    }
    /// 默认包的原始数组。
    pub fn default_to_array(&self) -> Collection<Collection<MessageValue>> {
        self.get_bag("default").as_bag().to_array().clone()
    }
    /// 默认包的 JSON。
    pub fn default_to_json(&self, options: MessageJsonOptions) -> Result<String> {
        self.get_bag("default").as_bag().to_json(options)
    }
    /// 默认包的缩进 JSON。
    pub fn default_to_pretty_json(&self, options: MessageJsonOptions) -> Result<String> {
        self.default_to_json(options.union(MessageJsonOptions::PRETTY_PRINT))
    }
    /// 默认包是否为空。
    pub fn default_is_empty(&self) -> bool {
        self.get_bag("default").as_bag().is_empty()
    }
    /// 默认包是否非空。
    pub fn default_is_not_empty(&self) -> bool {
        self.get_bag("default").as_bag().is_not_empty()
    }
    /// 默认包添加消息；缺失时只改临时包。
    pub fn default_add(
        &mut self,
        key: impl Into<ArrayKey>,
        message: impl Into<String>,
    ) -> Result<()> {
        self.get_bag_mut("default")
            .as_bag_mut()
            .add(key.into(), message.into())
    }
    /// 条件成立时向默认包添加消息；缺失时只改临时包。
    pub fn default_add_if(
        &mut self,
        condition: bool,
        key: impl Into<ArrayKey>,
        message: impl Into<String>,
    ) -> Result<()> {
        self.get_bag_mut("default")
            .as_bag_mut()
            .add_if(condition, key.into(), message.into())
    }
    /// 默认包合并消息；缺失时只改临时包。
    pub fn default_merge(&mut self, messages: &Collection<Collection<MessageValue>>) -> Result<()> {
        self.get_bag_mut("default").as_bag_mut().merge(messages)
    }
    /// 默认包移除键；缺失时只改临时包。
    pub fn default_forget(&mut self, key: &ArrayKey) {
        self.get_bag_mut("default").as_bag_mut().forget(key);
    }
    /// 默认包设置格式；缺失时只改临时包。
    pub fn default_set_format(&mut self, format: impl Into<String>) {
        self.get_bag_mut("default")
            .as_bag_mut()
            .set_format(format.into());
    }
}
impl fmt::Display for ViewErrorBag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bag = self.get_bag("default");
        let json = bag
            .as_bag()
            .to_json(MessageJsonOptions::NONE)
            .map_err(|_| fmt::Error)?;
        formatter.write_str(&json)
    }
}

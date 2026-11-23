use super::MessageBag;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{fmt, ops::Deref, sync::OnceLock};

/// 由会话传入视图的命名错误包；计数和默认方法只针对 default 包。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ViewErrorBag(IndexMap<String, MessageBag>);

impl ViewErrorBag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn has_bag(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
    /// 缺失包只返回空消息，不登记一个新包。
    pub fn get_bag(&self, key: &str) -> &MessageBag {
        static EMPTY: OnceLock<MessageBag> = OnceLock::new();
        self.0
            .get(key)
            .unwrap_or_else(|| EMPTY.get_or_init(MessageBag::default))
    }
    /// Rust 的显式可变借用；缺失包由调用方通过 put 添加。
    pub fn get_bag_mut(&mut self, key: &str) -> Option<&mut MessageBag> {
        self.0.get_mut(key)
    }
    pub fn get_bags(&self) -> &IndexMap<String, MessageBag> {
        &self.0
    }
    pub fn put(&mut self, key: impl Into<String>, bag: MessageBag) -> &mut Self {
        self.0.insert(key.into(), bag);
        self
    }
    pub fn count(&self) -> usize {
        self.get_bag("default").count()
    }
    pub fn any(&self) -> bool {
        self.count() > 0
    }
}

impl Deref for ViewErrorBag {
    type Target = MessageBag;
    fn deref(&self) -> &Self::Target {
        self.get_bag("default")
    }
}
impl fmt::Display for ViewErrorBag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get_bag("default").fmt(f)
    }
}

impl From<ViewErrorBag> for crate::view::ViewValue {
    fn from(errors: ViewErrorBag) -> Self {
        Self::Object(
            errors
                .0
                .into_iter()
                .map(|(name, bag)| {
                    let messages = bag
                        .messages()
                        .iter()
                        .map(|(key, values)| {
                            (
                                key.clone(),
                                Self::Array(values.iter().cloned().map(Self::String).collect()),
                            )
                        })
                        .collect();
                    (name, Self::Object(messages))
                })
                .collect(),
        )
    }
}

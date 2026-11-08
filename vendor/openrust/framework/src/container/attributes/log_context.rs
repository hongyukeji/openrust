//! 从同一容器的日志上下文仓储读取类型化构造参数。

use super::{
    Arc, AttributeParameter, AttributeValue, BoxFuture, ContextualAttribute, ParameterValue,
    Resolver, Result, container_error,
};
use crate::foundation::lifecycle::disposal;
use crate::logging::context::Repository;
use serde::de::DeserializeOwned;

/// 从当前作用域的日志上下文读取普通或隐藏值，按声明的 Rust 类型注入。
pub struct Context<T> {
    key: Arc<str>,
    hidden: bool,
    default: DefaultValue<T>,
}

enum DefaultValue<T> {
    Eager(ParameterValue),
    Lazy(Box<dyn Fn() -> Result<T> + Send + Sync>),
}

impl<T: DeserializeOwned + Send + Sync + 'static> Context<T> {
    /// 按精确键读取普通上下文。键缺失或为 null 时默认返回 null。
    pub fn new(key: impl Into<Arc<str>>) -> Self {
        Self {
            key: key.into(),
            hidden: false,
            default: DefaultValue::Eager(ParameterValue::null()),
        }
    }

    /// 改为只读取隐藏上下文。
    #[must_use]
    pub const fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    /// 为缺失键或 null 值指定默认值。
    #[must_use]
    pub fn with_default(mut self, value: T) -> Self {
        self.replace_default(DefaultValue::Eager(ParameterValue::new(value)));
        self
    }

    /// 在键缺失或值为 null 时调用默认工厂。工厂错误随参数解析返回。
    #[must_use]
    pub fn with_default_using(
        mut self,
        factory: impl Fn() -> Result<T> + Send + Sync + 'static,
    ) -> Self {
        self.replace_default(DefaultValue::Lazy(Box::new(factory)));
        self
    }

    /// 借用属性声明的精确键。
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// 返回此属性是否读取隐藏上下文。
    #[must_use]
    pub const fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// 借用静态默认值，未配置时为 `Some(null)`。
    /// 默认工厂在这里保持未调用并返回 `None`。
    #[must_use]
    pub const fn default_value(&self) -> Option<&ParameterValue> {
        match &self.default {
            DefaultValue::Eager(value) => Some(value),
            DefaultValue::Lazy(_) => None,
        }
    }

    fn replace_default(&mut self, default: DefaultValue<T>) {
        disposal::dispose(std::mem::replace(&mut self.default, default), None);
    }
}

impl<T> Drop for Context<T> {
    fn drop(&mut self) {
        let default = std::mem::replace(
            &mut self.default,
            DefaultValue::Eager(ParameterValue::null()),
        );
        disposal::dispose(default, None);
    }
}

impl<T: DeserializeOwned + Send + Sync + 'static> ContextualAttribute for Context<T> {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            let repository = resolver.resolve::<Repository>().await?;
            let stored = if self.hidden {
                repository.get_hidden(self.key())
            } else {
                repository.get(self.key())
            };
            let value = if let Some(stored) = stored {
                serde_json::from_value::<T>(stored)
                    .map(ParameterValue::new)
                    .map_err(|_| {
                        container_error(
                            "container_contextual_context_type_mismatch",
                            "日志上下文值与构造参数的声明类型不符",
                        )
                    })?
            } else {
                match &self.default {
                    DefaultValue::Eager(value) => value.clone(),
                    DefaultValue::Lazy(factory) => ParameterValue::new(factory()?),
                }
            };
            Ok(AttributeValue::One(value))
        })
    }
}

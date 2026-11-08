//! 容器内置上下文属性。

use super::super::{
    Arc, BoxFuture, ParameterOverrides, ParameterValue, ResolveRequest, Resolver, Result,
    ServiceKey, TypeId, container_error, lock,
};
use super::{AttributeParameter, AttributeValue, ContextualAttribute};
use crate::foundation::lifecycle::disposal;
use futures_util::stream::{self, BoxStream, StreamExt, TryStreamExt};
use serde::de::DeserializeOwned;
use std::marker::PhantomData;

/// 从当前应用配置仓储读取指定键，按声明的 Rust 类型注入。
pub struct Config<T> {
    key: Arc<str>,
    default: ParameterValue,
    output: PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned + Send + Sync + 'static> Config<T> {
    /// 创建配置属性。键不存在时返回 null，由调用位置决定是否继续选参。
    pub fn new(key: impl Into<Arc<str>>) -> Self {
        Self {
            key: key.into(),
            default: ParameterValue::null(),
            output: PhantomData,
        }
    }

    /// 为缺失键指定默认值。仓储中的显式 null 仍作为 null 返回。
    #[must_use]
    pub fn with_default(mut self, value: T) -> Self {
        let previous = std::mem::replace(&mut self.default, ParameterValue::new(value));
        disposal::dispose(previous, None);
        self
    }

    /// 返回当前属性声明的精确配置键。
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// 借用声明的默认值。未配置默认值时是 null。
    #[must_use]
    pub const fn default_value(&self) -> &ParameterValue {
        &self.default
    }
}

impl<T: DeserializeOwned + Send + Sync + 'static> ContextualAttribute for Config<T> {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            resolver
                .contextual_config::<T>(&self.key, &self.default)
                .map(AttributeValue::One)
        })
    }
}

/// 从当前容器解析指定实现或别名，按 `T` 注入同一个对象。
pub struct Give<T: ?Sized> {
    key: ServiceKey,
    parameters: ParameterOverrides,
    output: PhantomData<fn() -> Arc<T>>,
}

impl<T: ?Sized + Send + Sync + 'static> Give<T> {
    /// 使用给定服务键或别名。输出可为实现类型或该对象已声明的 trait 视图。
    pub fn new(key: ServiceKey) -> Self {
        Self {
            key,
            parameters: ParameterOverrides::new(),
            output: PhantomData,
        }
    }

    /// 设置解析该实现时使用的独立覆盖参数。
    #[must_use]
    pub fn with_parameters(mut self, parameters: ParameterOverrides) -> Self {
        std::mem::replace(&mut self.parameters, parameters).dispose_values();
        self
    }

    /// 借用属性查找时使用的键。
    #[must_use]
    pub const fn key(&self) -> &ServiceKey {
        &self.key
    }

    /// 借用仅传给所选实现的覆盖参数。
    #[must_use]
    pub const fn parameters(&self) -> &ParameterOverrides {
        &self.parameters
    }
}

impl<T: ?Sized> Drop for Give<T> {
    fn drop(&mut self) {
        std::mem::take(&mut self.parameters).dispose_values();
    }
}

impl<T: ?Sized + Send + Sync + 'static> ContextualAttribute for Give<T> {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        let request = ResolveRequest::Typed {
            lookup: Some(self.key.clone()),
            target: ServiceKey::typed::<T>(),
            erased_output: TypeId::of::<Arc<T>>(),
        };
        Resolver::resolve_mapped(
            move || (Ok(resolver), ()),
            request,
            self.parameters.clone(),
            |value| {
                if value.downcast_ref::<Arc<T>>().is_none() {
                    return Err(container_error(
                        "service_type_mismatch",
                        "服务的实际输出不符合声明类型",
                    ));
                }
                Ok(AttributeValue::One(ParameterValue::from_erased(value)))
            },
        )
    }
}

/// 取得指定标签的惰性服务序列。构造参数声明为 [`Tagged<T>`]。
pub struct Tag<T: ?Sized> {
    tag: Arc<str>,
    output: PhantomData<fn() -> Arc<T>>,
}

impl<T: ?Sized + Send + Sync + 'static> Tag<T> {
    /// 使用精确的标签名，保留登记顺序和重复键。
    pub fn new(tag: impl Into<Arc<str>>) -> Self {
        Self {
            tag: tag.into(),
            output: PhantomData,
        }
    }

    /// 返回属性声明的精确标签名。
    #[must_use]
    pub fn name(&self) -> &str {
        &self.tag
    }
}

impl<T: ?Sized + Send + Sync + 'static> ContextualAttribute for Tag<T> {
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        mut resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            let container = resolver.container()?;
            let length = {
                let state = lock(&container.inner.state);
                resolver.check_open(&state)?;
                state
                    .contextual
                    .tag_entry(&self.tag)
                    .map(<[ServiceKey]>::len)
            };
            // 后续迭代只复用作用域及构造上下文，不能延长本次原工厂实参的寿命。
            resolver.clear_factory_inputs();
            let tagged = Tagged::<T> {
                length: length.unwrap_or_default(),
                source: length.map(|_| TaggedSource {
                    tag: Arc::clone(&self.tag),
                    resolver,
                }),
                output: PhantomData,
            };
            Ok(AttributeValue::One(ParameterValue::new(tagged)))
        })
    }
}

struct TaggedSource {
    tag: Arc<str>,
    resolver: Resolver,
}

/// 可重新迭代的标签服务序列，解析沿用创建它时的容器及作用域。
pub struct Tagged<T: ?Sized> {
    length: usize,
    source: Option<TaggedSource>,
    output: PhantomData<fn() -> Arc<T>>,
}

impl<T: ?Sized + Send + Sync + 'static> Tagged<T> {
    /// 返回取得该序列时的登记数量，包含重复项。
    #[must_use]
    pub const fn len(&self) -> usize {
        self.length
    }

    /// 判断取得该序列时的登记数量是否为零。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// 每轮开始时读取当时的标签键，然后逐个解析。解析失败会结束当前这一轮。
    ///
    /// 取得序列时不存在的标签始终为空。已登记的标签可在后续轮次看到新增键。
    pub fn stream(&self) -> BoxStream<'_, Result<Arc<T>>> {
        let Some(source) = &self.source else {
            return stream::empty().boxed();
        };
        stream::once(async move {
            let container = source.resolver.container()?;
            let keys = {
                let state = lock(&container.inner.state);
                source.resolver.check_open(&state)?;
                state.contextual.tagged_keys(&source.tag).to_vec()
            };
            Ok::<_, crate::Error>(stream::try_unfold(
                keys.into_iter(),
                move |mut keys| async move {
                    let Some(key) = keys.next() else {
                        return Ok(None);
                    };
                    let service = source.resolver.resolve_key::<T>(key).await?;
                    Ok(Some((service, keys)))
                },
            ))
        })
        .try_flatten()
        .boxed()
    }
}

#[cfg(test)]
#[path = "builtin_tests.rs"]
mod tests;

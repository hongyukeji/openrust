//! 上下文绑定的原键登记、参数值和有序标签。

use super::{
    Arc, BoxFuture, Container, ContainerBuilder, Factory, HashMap, ParameterValue, Resolver,
    Result, ServiceKey, TypeId, Value, container_error, lock,
};
use crate::config::ConfigError;
use serde::de::DeserializeOwned;

type ParameterFactory =
    Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<ParameterValue>> + Send + Sync>;
type VariadicFactory =
    Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<Vec<Value>>> + Send + Sync>;

pub(super) enum ContextualImplementation {
    Service(ServiceKey),
    Factory {
        factory: Factory,
        output: TypeId,
    },
    FactoryMany {
        factory: VariadicFactory,
        output: TypeId,
    },
    Services(Vec<ServiceKey>),
    Tagged(Arc<str>),
    Parameter(ParameterValue),
    ParameterFactory(ParameterFactory),
}

enum ContextualNeed {
    Service(ServiceKey),
    Parameter(Arc<str>),
}

#[derive(Default)]
struct ConsumerBindings {
    services: HashMap<ServiceKey, Arc<ContextualImplementation>>,
    parameters: HashMap<Arc<str>, Arc<ContextualImplementation>>,
}

#[derive(Default)]
pub(super) struct ContextualBindings {
    consumers: HashMap<ServiceKey, ConsumerBindings>,
    tags: HashMap<Arc<str>, Vec<ServiceKey>>,
}

impl ContextualBindings {
    pub(super) fn get_service(
        &self,
        consumer: &ServiceKey,
        service: &ServiceKey,
    ) -> Option<&Arc<ContextualImplementation>> {
        self.consumers.get(consumer)?.services.get(service)
    }

    pub(super) fn get_parameter(
        &self,
        consumer: &ServiceKey,
        parameter: &str,
    ) -> Option<&Arc<ContextualImplementation>> {
        self.consumers.get(consumer)?.parameters.get(parameter)
    }

    pub(super) fn tagged_keys(&self, tag: &str) -> &[ServiceKey] {
        self.tag_entry(tag).unwrap_or_default()
    }

    pub(super) fn tag_entry(&self, tag: &str) -> Option<&[ServiceKey]> {
        self.tags.get(tag).map(Vec::as_slice)
    }

    fn replace(
        &mut self,
        consumers: Vec<ServiceKey>,
        need: &ContextualNeed,
        implementation: Option<&Arc<ContextualImplementation>>,
    ) -> Vec<Arc<ContextualImplementation>> {
        consumers
            .into_iter()
            .filter_map(|consumer| {
                if let Some(implementation) = implementation {
                    let bindings = self.consumers.entry(consumer).or_default();
                    match need {
                        ContextualNeed::Service(key) => bindings
                            .services
                            .insert(key.clone(), Arc::clone(implementation)),
                        ContextualNeed::Parameter(name) => bindings
                            .parameters
                            .insert(Arc::clone(name), Arc::clone(implementation)),
                    }
                } else {
                    let bindings = self.consumers.get_mut(&consumer)?;
                    match need {
                        ContextualNeed::Service(key) => bindings.services.remove(key),
                        ContextualNeed::Parameter(name) => bindings.parameters.remove(name),
                    }
                }
            })
            .collect()
    }

    fn tag(&mut self, services: &[ServiceKey], tags: Vec<Arc<str>>) {
        for tag in tags {
            self.tags
                .entry(tag)
                .or_default()
                .extend_from_slice(services);
        }
    }
}

enum Registration<'a> {
    Builder(&'a mut ContainerBuilder),
    Container(&'a Container),
}

/// 为一组构造消费者配置其依赖的上下文实现。
///
/// 在 `when` 时保存消费者的当前别名目标，在 `give` 时解析所需服务的别名。
/// 规则只用于该消费者实际构造时的依赖解析。
pub struct ContextualBindingBuilder<'a> {
    owner: Registration<'a>,
    consumers: Vec<ServiceKey>,
}

impl<'a> ContextualBindingBuilder<'a> {
    /// 选择需要的服务，键可来自类型、类名或已登记的命名服务。
    #[must_use]
    pub fn needs(self, service: ServiceKey) -> ContextualServiceBinding<'a> {
        ContextualServiceBinding {
            binding: self,
            service,
        }
    }

    /// 选择构造参数的精确名称，例如 `timeout` 对应 Laravel 的 `$timeout`。
    #[must_use]
    pub fn needs_parameter(self, name: impl Into<Arc<str>>) -> ContextualParameterBinding<'a> {
        ContextualParameterBinding {
            binding: self,
            name: name.into(),
        }
    }

    fn install(
        self,
        mut need: ContextualNeed,
        implementation: Option<ContextualImplementation>,
    ) -> Result<()> {
        let implementation = implementation.map(Arc::new);
        let retired = match self.owner {
            Registration::Builder(builder) => {
                if let ContextualNeed::Service(key) = &mut need {
                    *key = builder
                        .aliases
                        .resolve(&builder.classes.canonical_key(key))?;
                }
                builder
                    .contextual
                    .replace(self.consumers, &need, implementation.as_ref())
            }
            Registration::Container(container) => {
                let mut state = lock(&container.inner.state);
                if state.terminated {
                    return Err(container_error("container_closed", "容器已关闭"));
                }
                if let ContextualNeed::Service(key) = &mut need {
                    *key = state.aliases.resolve(&state.classes.canonical_key(key))?;
                }
                let retired =
                    state
                        .contextual
                        .replace(self.consumers, &need, implementation.as_ref());
                let changes = Arc::clone(&state.binding_changes);
                drop(state);
                changes.notify_waiters();
                retired
            }
        };
        // 被替换的闭包和值可以重入容器，须在 State 锁释放后退役。
        drop(retired);
        Ok(())
    }
}

/// 为指定消费类所需的服务登记上下文实现。
pub struct ContextualServiceBinding<'a> {
    binding: ContextualBindingBuilder<'a>,
    service: ServiceKey,
}

impl ContextualServiceBinding<'_> {
    /// 通过普通解析管线取得实现。实现键在使用时才跟随别名。
    pub fn give(self, implementation: ServiceKey) -> Result<()> {
        self.install(ContextualImplementation::Service(implementation))
    }

    /// 在构造时调用显式工厂，登记其输出类型供解析预检。
    pub fn give_factory<T, F>(self, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        let factory: Factory = Arc::new(move |resolver| {
            let future = factory(resolver);
            Box::pin(async move { future.await.map(super::erase) })
        });
        self.install(ContextualImplementation::Factory {
            factory,
            output: TypeId::of::<Arc<T>>(),
        })
    }

    /// 按给定顺序解析可变参数服务，保留重复键并允许空序列。
    pub fn give_many(self, implementations: impl IntoIterator<Item = ServiceKey>) -> Result<()> {
        self.install(ContextualImplementation::Services(
            implementations.into_iter().collect(),
        ))
    }

    /// 在构造时调用工厂生成有序服务列表，登记元素类型供变参解析预检。
    pub fn give_variadic_factory<T, F>(self, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Vec<Arc<T>>>> + Send + Sync + 'static,
    {
        let factory: VariadicFactory = Arc::new(move |resolver| {
            let future = factory(resolver);
            Box::pin(async move {
                let values = future.await?;
                Ok(values.into_iter().map(super::erase).collect())
            })
        });
        self.install(ContextualImplementation::FactoryMany {
            factory,
            output: TypeId::of::<Arc<T>>(),
        })
    }

    /// 按登记顺序解析指定标签的可变参数服务，未登记标签产生空序列。
    pub fn give_tagged(self, tag: impl Into<Arc<str>>) -> Result<()> {
        self.install(ContextualImplementation::Tagged(tag.into()))
    }

    fn install(self, implementation: ContextualImplementation) -> Result<()> {
        self.binding
            .install(ContextualNeed::Service(self.service), Some(implementation))
    }
}

/// 为指定消费类的具名构造参数登记上下文值。
pub struct ContextualParameterBinding<'a> {
    binding: ContextualBindingBuilder<'a>,
    name: Arc<str>,
}

impl ContextualParameterBinding<'_> {
    /// 登记类型化值。与 Laravel 的 `give(null)` 一致，直接传入 null 会移除本项规则。
    pub fn give_value(self, value: ParameterValue) -> Result<()> {
        let implementation =
            (!value.is_null()).then_some(ContextualImplementation::Parameter(value));
        self.binding
            .install(ContextualNeed::Parameter(self.name), implementation)
    }

    /// 按需生成参数值。工厂返回的显式 null 会交给参数校验，保留该值而非采用默认值。
    pub fn give_parameter_factory<F>(self, factory: F) -> Result<()>
    where
        F: Fn(Resolver) -> BoxFuture<'static, Result<ParameterValue>> + Send + Sync + 'static,
    {
        self.binding.install(
            ContextualNeed::Parameter(self.name),
            Some(ContextualImplementation::ParameterFactory(Arc::new(
                factory,
            ))),
        )
    }

    /// 从当前配置仓储的原始值视图读取配置，并反序列化为参数声明的 Rust 类型 `T`。
    ///
    /// 仅在配置键缺失时使用 `default`。缺失且无默认值或配置显式为 null 时注入 null。
    pub fn give_config<T>(self, key: impl Into<Arc<str>>, default: Option<T>) -> Result<()>
    where
        T: DeserializeOwned + Send + Sync + 'static,
    {
        let key = key.into();
        let default = default.map_or_else(ParameterValue::null, ParameterValue::new);
        self.give_parameter_factory(move |resolver| {
            let key = Arc::clone(&key);
            let default = default.clone();
            Box::pin(async move { resolver.contextual_config::<T>(&key, &default) })
        })
    }

    /// 按登记顺序解析指定标签，将结果作为 `Vec<Arc<T>>` 参数注入。
    pub fn give_tagged<T: ?Sized + Send + Sync + 'static>(
        self,
        tag: impl Into<Arc<str>>,
    ) -> Result<()> {
        let tag = tag.into();
        self.give_parameter_factory(move |resolver| {
            let tag = Arc::clone(&tag);
            Box::pin(async move {
                let container = resolver.container()?;
                let keys = {
                    let state = lock(&container.inner.state);
                    resolver.check_open(&state)?;
                    state.contextual.tagged_keys(&tag).to_vec()
                };
                let mut values = Vec::with_capacity(keys.len());
                for key in keys {
                    values.push(resolver.resolve_key::<T>(key).await?);
                }
                Ok(ParameterValue::new(values))
            })
        })
    }
}

impl Resolver {
    pub(super) fn contextual_config<T: DeserializeOwned + Send + Sync + 'static>(
        &self,
        key: &str,
        default: &ParameterValue,
    ) -> Result<ParameterValue> {
        let container = self.container()?;
        let config = container
            .inner
            .config
            .as_ref()
            .ok_or_else(|| ConfigError::Invalid("服务容器未配置配置仓储".into()))?;
        match config.get(key)? {
            None => Ok(default.clone()),
            Some(serde_json::Value::Null) => Ok(ParameterValue::null()),
            Some(value) => serde_json::from_value::<T>(value)
                .map(ParameterValue::new)
                .map_err(|_| {
                    container_error(
                        "container_contextual_config_type_mismatch",
                        "配置值与构造参数的声明类型不符",
                    )
                }),
        }
    }
}

impl ContainerBuilder {
    /// 开始配置一个消费类的上下文依赖。
    pub fn when(&mut self, consumer: ServiceKey) -> Result<ContextualBindingBuilder<'_>> {
        self.when_many([consumer])
    }

    /// 为多个消费类应用同一条上下文规则。
    pub fn when_many(
        &mut self,
        consumers: impl IntoIterator<Item = ServiceKey>,
    ) -> Result<ContextualBindingBuilder<'_>> {
        let consumers = consumers
            .into_iter()
            .map(|key| self.aliases.resolve(&self.classes.canonical_key_owned(key)))
            .collect::<Result<_>>()?;
        Ok(ContextualBindingBuilder {
            owner: Registration::Builder(self),
            consumers,
        })
    }

    /// 按输入顺序将服务追加到每个标签。解析标签时按当时的别名跟随这些键。
    pub fn tag(
        &mut self,
        services: impl IntoIterator<Item = ServiceKey>,
        tags: impl IntoIterator<Item = impl Into<Arc<str>>>,
    ) {
        let services: Vec<_> = services.into_iter().collect();
        self.contextual
            .tag(&services, tags.into_iter().map(Into::into).collect());
    }
}

impl Container {
    /// 开始配置一个消费类的上下文依赖，适用于 Provider 注册或运行时扩展。
    pub fn when(&self, consumer: ServiceKey) -> Result<ContextualBindingBuilder<'_>> {
        self.when_many([consumer])
    }

    /// 为多个消费类应用同一条上下文规则。
    pub fn when_many(
        &self,
        consumers: impl IntoIterator<Item = ServiceKey>,
    ) -> Result<ContextualBindingBuilder<'_>> {
        let consumers: Vec<_> = consumers.into_iter().collect();
        let consumers = {
            let state = lock(&self.inner.state);
            if state.terminated {
                return Err(container_error("container_closed", "容器已关闭"));
            }
            consumers
                .into_iter()
                .map(|key| {
                    state
                        .aliases
                        .resolve(&state.classes.canonical_key_owned(key))
                })
                .collect::<Result<_>>()?
        };
        Ok(ContextualBindingBuilder {
            owner: Registration::Container(self),
            consumers,
        })
    }

    /// 按输入顺序将服务追加到每个标签。未知标签在首次登记时建立。
    pub fn tag(
        &self,
        services: impl IntoIterator<Item = ServiceKey>,
        tags: impl IntoIterator<Item = impl Into<Arc<str>>>,
    ) -> Result<()> {
        let services: Vec<_> = services.into_iter().collect();
        let tags = tags.into_iter().map(Into::into).collect();
        let mut state = lock(&self.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        state.contextual.tag(&services, tags);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Weak,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn consumer_alias_is_fixed_at_when_and_dependency_alias_at_give() -> Result<()> {
        let container = ContainerBuilder::new().build();
        let consumer_alias = ServiceKey::named("consumer");
        let consumer = ServiceKey::named("original_consumer");
        let new_consumer = ServiceKey::named("new_consumer");
        let need_alias = ServiceKey::named("need");
        let new_need = ServiceKey::named("new_need");
        let implementation = ServiceKey::named("implementation");
        container.alias(consumer.clone(), consumer_alias.clone())?;
        container.alias(ServiceKey::named("original_need"), need_alias.clone())?;

        let rule = container
            .when(consumer_alias.clone())?
            .needs(need_alias.clone());
        container.alias(new_consumer.clone(), consumer_alias)?;
        container.alias(new_need.clone(), need_alias.clone())?;
        rule.give(implementation.clone())?;

        let state = lock(&container.inner.state);
        assert!(matches!(
            state.contextual.get_service(&consumer, &new_need).map(Arc::as_ref),
            Some(ContextualImplementation::Service(key)) if key == &implementation
        ));
        assert!(
            state
                .contextual
                .get_service(&consumer, &need_alias)
                .is_none()
        );
        assert!(
            state
                .contextual
                .get_service(&new_consumer, &new_need)
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn literal_null_retires_previous_rule_outside_the_state_lock() -> Result<()> {
        struct CheckOnDrop(Weak<super::super::Inner>, Arc<AtomicBool>);

        impl Drop for CheckOnDrop {
            fn drop(&mut self) {
                let unlocked = self
                    .0
                    .upgrade()
                    .is_some_and(|inner| inner.state.try_lock().is_ok());
                self.1.store(unlocked, Ordering::Relaxed);
            }
        }

        let container = ContainerBuilder::new().build();
        let consumer = ServiceKey::typed::<String>();
        let unlocked = Arc::new(AtomicBool::new(false));
        container
            .when(consumer.clone())?
            .needs_parameter("value")
            .give_value(ParameterValue::new(CheckOnDrop(
                Arc::downgrade(&container.inner),
                Arc::clone(&unlocked),
            )))?;
        assert!(!unlocked.load(Ordering::Relaxed));
        container
            .when(consumer.clone())?
            .needs_parameter("value")
            .give_value(ParameterValue::null())?;
        assert!(unlocked.load(Ordering::Relaxed));
        assert!(
            lock(&container.inner.state)
                .contextual
                .get_parameter(&consumer, "value")
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn builder_and_live_tags_share_order_and_retain_duplicate_raw_keys() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        let first = ServiceKey::named("first");
        let second = ServiceKey::named("second");
        builder.tag([first.clone()], ["tag"]);
        let container = builder.build();
        container.tag([second.clone(), first.clone()], ["tag", "other"])?;
        container.alias(second.clone(), first.clone())?;

        let state = lock(&container.inner.state);
        assert_eq!(
            state.contextual.tagged_keys("tag"),
            [first.clone(), second.clone(), first.clone()]
        );
        assert_eq!(state.contextual.tagged_keys("other"), [second, first]);
        assert_eq!(state.contextual.tagged_keys("missing"), []);
        Ok(())
    }
}

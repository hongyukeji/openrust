//! 编译期构造属性、上下文处理器和后置属性回调。

use super::{
    Arc, BoxFuture, Container, ContainerBuilder, HashMap, Inner, ParameterValue, Resolver, Result,
    ServiceKey, Weak, container_error, lock,
};
use crate::foundation::lifecycle::{
    CallbackDisposal, TaskExecutionFailure, callback_future, disposal,
};
use std::collections::VecDeque;

mod builtin;
pub use builtin::{Config, Give, Tag, Tagged};
mod managers;
pub use managers::{Cache, Database, DbAttribute as DB, Storage};

/// 当前构造参数的属性解析信息。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttributeParameter {
    name: &'static str,
    variadic: bool,
    new_instance: bool,
}

impl AttributeParameter {
    pub(super) fn new(name: &'static str, variadic: bool, new_instance: bool) -> Self {
        Self {
            name,
            variadic,
            new_instance,
        }
    }

    /// 返回参数的精确名称。
    #[must_use]
    pub fn name(self) -> &'static str {
        self.name
    }

    /// 表明当前参数能接收零项或多项值。
    #[must_use]
    pub fn is_variadic(self) -> bool {
        self.variadic
    }

    /// 表明参数属于 `SelfBuilding::new_instance` 方法。
    #[must_use]
    pub fn is_new_instance(self) -> bool {
        self.new_instance
    }
}

/// 属性解析的结果，或者属性回调观察的实参或对象。
#[derive(Clone, Debug)]
pub enum AttributeValue {
    /// 单项值，包括显式 null。
    One(ParameterValue),
    /// 按顺序保留的变参值。
    Many(AttributeArguments),
}

impl AttributeValue {
    /// 创建有序变参结果。仅构造参数可从属性直接接收多项。
    pub fn many(values: impl IntoIterator<Item = ParameterValue>) -> Self {
        let mut arguments = AttributeArguments(VecDeque::new());
        arguments.0.extend(values);
        Self::Many(arguments)
    }
}

/// 属性派发持有的有序变参值。
#[derive(Clone, Debug)]
pub struct AttributeArguments(VecDeque<ParameterValue>);

impl AttributeArguments {
    /// 借用本次变参中的全部值。
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ParameterValue> {
        self.0.iter()
    }

    pub(super) fn pop_front(&mut self) -> Option<ParameterValue> {
        self.0.pop_front()
    }
}

impl Drop for AttributeArguments {
    fn drop(&mut self) {
        // 已收到的属性结果可能尚未拆入单项槽，取消时仍须逐项隔离析构。
        for value in self.0.drain(..) {
            disposal::dispose(value, None);
        }
    }
}

/// 可参与参数上下文解析的属性。
///
/// 容器登记的处理器优先于属性自身的解析方法。属性实例由
/// [`ClassAttribute`] 在每次调用前创建。
pub trait ContextualAttribute: Send + Sync + 'static {
    /// 解析当前参数。默认要求通过容器登记专用处理器。
    fn resolve(
        self: Arc<Self>,
        _parameter: AttributeParameter,
        _resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async {
            Err(container_error(
                "container_attribute_handler_missing",
                "上下文属性缺少解析处理器",
            ))
        })
    }

    /// 在解析参数或构造对象后运行属性自己的回调。默认无动作。
    fn after(
        self: Arc<Self>,
        _value: AttributeValue,
        _resolver: Resolver,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

type Instantiate = dyn Fn() -> Result<ParameterValue> + Send + Sync;

/// 可附着于编译类或其构造参数的属性声明。
///
/// 使用类型身份匹配处理器。每次执行属性逻辑都通过声明的工厂建立新的实例。
#[derive(Clone)]
pub struct ClassAttribute {
    key: ServiceKey,
    instantiate: Arc<Instantiate>,
    resolver: Option<Arc<AttributeHandler>>,
    after: Option<Arc<AttributeAfter>>,
}

impl ClassAttribute {
    /// 声明只参与后置回调的标记属性。
    pub fn marker<A>(factory: impl Fn() -> Result<A> + Send + Sync + 'static) -> Self
    where
        A: Send + Sync + 'static,
    {
        Self {
            key: ServiceKey::typed::<A>(),
            instantiate: Arc::new(move || factory().map(ParameterValue::new)),
            resolver: None,
            after: None,
        }
    }

    /// 声明可以解析参数并执行自身后置逻辑的属性。
    pub fn contextual<A>(factory: impl Fn() -> Result<A> + Send + Sync + 'static) -> Self
    where
        A: ContextualAttribute,
    {
        Self {
            resolver: Some(AttributeHandler::typed(A::resolve)),
            after: Some(AttributeAfter::new(|event, resolver| {
                match event.attribute().downcast::<A>() {
                    Ok(attribute) => attribute.after(event.value, resolver),
                    Err(error) => Box::pin(async move { Err(error) }),
                }
            })),
            ..Self::marker(factory)
        }
    }

    pub(super) fn key(&self) -> &ServiceKey {
        &self.key
    }

    pub(super) fn is_contextual(&self) -> bool {
        self.resolver.is_some()
    }

    pub(super) fn instantiate(&self) -> Result<ParameterValue> {
        (self.instantiate)()
    }

    pub(super) fn resolver(&self) -> Option<Arc<AttributeHandler>> {
        self.resolver.clone()
    }

    pub(super) fn after(&self) -> Option<Arc<AttributeAfter>> {
        self.after.clone()
    }
}

/// 后置属性回调收到的新属性实例及对应值。
#[derive(Clone)]
pub struct AttributeEvent {
    attribute: ParameterValue,
    value: AttributeValue,
    container: Weak<Inner>,
}

impl AttributeEvent {
    pub(super) fn new(
        attribute: ParameterValue,
        value: AttributeValue,
        inner: &Arc<Inner>,
    ) -> Self {
        Self {
            attribute,
            value,
            container: Arc::downgrade(inner),
        }
    }

    /// 借用此回调的属性实例。使用 [`ParameterValue::downcast`] 按声明的属性类型读取。
    #[must_use]
    pub fn attribute(&self) -> &ParameterValue {
        &self.attribute
    }

    /// 借用本次已解析的参数或构造对象。
    #[must_use]
    pub fn value(&self) -> &AttributeValue {
        &self.value
    }

    /// 读取参数或构造对象的已编译服务视图。
    /// 普通值、空值和整组变参返回 `None`。
    pub fn service<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Option<Arc<T>>> {
        let AttributeValue::One(value) = &self.value else {
            return Ok(None);
        };
        let Some(inner) = self.container.upgrade() else {
            return Err(container_error("container_closed", "容器已关闭"));
        };
        let classes = Arc::clone(&lock(&inner.state).classes);
        super::resolving_hooks::object_view(&classes, value.service_value())
    }

    /// 访问当前应用的登记入口。事件仅持有弱引用，嵌套解析使用回调接收的 `Resolver`。
    pub fn container(&self) -> Result<Container> {
        let inner = self
            .container
            .upgrade()
            .ok_or_else(|| container_error("container_closed", "容器已关闭"))?;
        let container = Container { inner };
        if container.is_terminated() {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        Ok(container)
    }
}

type ResolveAttribute = dyn Fn(ParameterValue, AttributeParameter, Resolver) -> BoxFuture<'static, Result<AttributeValue>>
    + Send
    + Sync;

pub(super) struct AttributeHandler(Box<ResolveAttribute>);

impl AttributeHandler {
    fn typed<A>(
        handler: impl Fn(
            Arc<A>,
            AttributeParameter,
            Resolver,
        ) -> BoxFuture<'static, Result<AttributeValue>>
        + Send
        + Sync
        + 'static,
    ) -> Arc<Self>
    where
        A: Send + Sync + 'static,
    {
        Arc::new(Self(Box::new(
            move |attribute, parameter, resolver| match attribute.downcast::<A>() {
                Ok(attribute) => handler(attribute, parameter, resolver),
                Err(error) => Box::pin(async move { Err(error) }),
            },
        )))
    }

    pub(super) fn invoke(
        self: Arc<Self>,
        attr: ParameterValue,
        param: AttributeParameter,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(callback_future(
            resolver.callback_disposal(),
            self,
            resolver,
            Ok((attr, param)),
            None,
            |handler, resolver, (attribute, parameter)| (handler.0)(attribute, parameter, resolver),
        ))
    }
}

type AfterAttribute =
    dyn Fn(AttributeEvent, Resolver) -> BoxFuture<'static, Result<()>> + Send + Sync;

pub(super) struct AttributeAfter(Box<AfterAttribute>);

impl AttributeAfter {
    fn new(
        callback: impl Fn(AttributeEvent, Resolver) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Arc<Self> {
        Arc::new(Self(Box::new(callback)))
    }

    pub(super) fn invoke(
        self: Arc<Self>,
        event: AttributeEvent,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(callback_future(
            resolver.callback_disposal(),
            self,
            resolver,
            Ok(event),
            None,
            |callback, resolver, event| (callback.0)(event, resolver),
        ))
    }
}

#[derive(Default)]
pub(super) struct AttributeRegistry {
    handlers: HashMap<ServiceKey, Arc<AttributeHandler>>,
    after: Vec<(ServiceKey, Vec<Arc<AttributeAfter>>)>,
}

impl AttributeRegistry {
    pub(super) fn handler(&self, key: &ServiceKey) -> Option<Arc<AttributeHandler>> {
        self.handlers.get(key).cloned()
    }

    pub(super) fn matching(
        &self,
        attribute: &ServiceKey,
        mut object_matches: impl FnMut(&ServiceKey) -> bool,
    ) -> Vec<Arc<AttributeAfter>> {
        self.after
            .iter()
            .filter(|(key, _)| key == attribute || object_matches(key))
            .flat_map(|(_, callbacks)| callbacks.iter().cloned())
            .collect()
    }

    fn register_after(&mut self, key: ServiceKey, callback: Arc<AttributeAfter>) {
        if let Some((_, callbacks)) = self.after.iter_mut().find(|(known, _)| known == &key) {
            callbacks.push(callback);
        } else {
            self.after.push((key, vec![callback]));
        }
    }
}

impl ContainerBuilder {
    /// 登记属性类型的上下文解析处理器。同类型后登记的处理器优先。
    pub fn when_has_attribute<A: Send + Sync + 'static>(
        &mut self,
        handler: impl Fn(
            Arc<A>,
            AttributeParameter,
            Resolver,
        ) -> BoxFuture<'static, Result<AttributeValue>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        let previous = self
            .attributes
            .handlers
            .insert(ServiceKey::typed::<A>(), AttributeHandler::typed(handler));
        retire_registration(&CallbackDisposal::new(None, None), previous, Ok(()))
    }

    /// 按属性类型或已解析对象的编译类型登记后置回调。
    pub fn after_resolving_attribute<A: ?Sized + 'static>(
        &mut self,
        callback: impl Fn(AttributeEvent, Resolver) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.attributes
            .register_after(ServiceKey::typed::<A>(), AttributeAfter::new(callback));
        Ok(())
    }
}

impl Container {
    /// 为当前应用登记属性类型的上下文处理器。同类型旧处理器在锁外退役。
    pub fn when_has_attribute<A: Send + Sync + 'static>(
        &self,
        handler: impl Fn(
            Arc<A>,
            AttributeParameter,
            Resolver,
        ) -> BoxFuture<'static, Result<AttributeValue>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        let handler = AttributeHandler::typed(handler);
        let mut state = lock(&self.inner.state);
        let disposal = CallbackDisposal::new(None, Some(state.root_resources.registrar()));
        if state.terminated {
            drop(state);
            return retire_registration(
                &disposal,
                handler,
                Err(container_error("container_closed", "容器已关闭")),
            );
        }
        let previous = state
            .attributes
            .handlers
            .insert(ServiceKey::typed::<A>(), handler);
        drop(state);
        retire_registration(&disposal, previous, Ok(()))
    }

    /// 为当前应用按属性类型或已解析对象的编译类型登记后置回调。
    pub fn after_resolving_attribute<A: ?Sized + 'static>(
        &self,
        callback: impl Fn(AttributeEvent, Resolver) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        let callback = AttributeAfter::new(callback);
        let mut state = lock(&self.inner.state);
        if state.terminated {
            let disposal = CallbackDisposal::new(None, Some(state.root_resources.registrar()));
            drop(state);
            return retire_registration(
                &disposal,
                callback,
                Err(container_error("container_closed", "容器已关闭")),
            );
        }
        state
            .attributes
            .register_after(ServiceKey::typed::<A>(), callback);
        Ok(())
    }
}

pub(super) fn retire_registration<T>(
    disposal: &CallbackDisposal,
    registration: T,
    result: Result<()>,
) -> Result<()> {
    if !disposal.run(|| disposal::dispose(registration, None)).seen {
        return result;
    }
    let cleanup = container_error("resource_task_interrupted", "容器回调登记清理异常");
    match result {
        Ok(()) => Err(cleanup),
        Err(primary) => Err(TaskExecutionFailure::error(primary, cleanup)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct Marker(usize);

    #[test]
    fn marker_creates_a_fresh_typed_instance_for_each_callback() -> Result<()> {
        let count = Arc::new(AtomicUsize::new(0));
        let attribute =
            ClassAttribute::marker(move || Ok(Marker(count.fetch_add(1, Ordering::Relaxed))));
        assert_eq!(attribute.key(), &ServiceKey::typed::<Marker>());
        assert!(!attribute.is_contextual());
        let first = attribute.instantiate()?.downcast::<Marker>()?;
        let second = attribute.instantiate()?.downcast::<Marker>()?;
        assert_eq!((first.0, second.0), (0, 1));
        Ok(())
    }

    struct ObserveRetirement {
        inner: Weak<Inner>,
        dropped_without_state_lock: Arc<AtomicBool>,
    }

    impl Drop for ObserveRetirement {
        fn drop(&mut self) {
            if let Some(inner) = self.inner.upgrade() {
                self.dropped_without_state_lock
                    .store(inner.state.try_lock().is_ok(), Ordering::Release);
            }
        }
    }

    #[test]
    fn replacing_live_attribute_handler_retires_capture_outside_state_lock() -> Result<()> {
        let container = ContainerBuilder::new().build();
        let unlocked = Arc::new(AtomicBool::new(false));
        let probe = ObserveRetirement {
            inner: Arc::downgrade(&container.inner),
            dropped_without_state_lock: Arc::clone(&unlocked),
        };
        container.when_has_attribute::<Marker>(move |_, _, _| {
            let _ = &probe;
            Box::pin(async { Ok(AttributeValue::One(ParameterValue::null())) })
        })?;
        container.when_has_attribute::<Marker>(|_, _, _| {
            Box::pin(async { Ok(AttributeValue::many([])) })
        })?;
        assert!(unlocked.load(Ordering::Acquire));
        Ok(())
    }
}

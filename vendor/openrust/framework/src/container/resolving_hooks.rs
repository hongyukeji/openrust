//! Laravel 容器解析回调的登记、类型匹配与派发。

use super::{
    Arc, BoxFuture, ClassRegistry, Container, ContainerBuilder, Inner, ParameterOverrides,
    ParameterView, ResolveRequest, Resolver, Result, ServiceKey, Value, Weak, container_error,
    lock,
};
use crate::foundation::lifecycle::{CallbackDisposal, callback_future, disposal};

/// 当前解析阶段的规范服务键及已构造对象视图。
///
/// 在 `before_resolving` 阶段还没有对象。使用 [`ResolutionEvent::parameters`]
/// 读取本次查找的实参，此时构造覆盖栈仍处于上一次活动调用。
#[derive(Clone)]
pub struct ResolutionEvent {
    key: ServiceKey,
    value: Option<Value>,
    arguments: Option<ParameterView>,
    classes: Arc<ClassRegistry>,
    container: Weak<Inner>,
}

impl ResolutionEvent {
    /// 返回本次查找在开始派发前确定的服务键。
    #[must_use]
    pub fn key(&self) -> &ServiceKey {
        &self.key
    }

    /// 复制本次解析的显式实参。对应调用结束后的事件视图返回空集合。
    pub fn parameters(&self) -> ParameterOverrides {
        self.arguments
            .as_ref()
            .map_or_else(ParameterOverrides::new, ParameterView::snapshot)
    }

    /// 访问当前应用容器，以便登记绑定或其它解析回调。
    ///
    /// 事件自身以弱引用关联容器。嵌套服务解析请使用回调接收的 `Resolver`，以保留当前作用域。
    pub fn container(&self) -> Result<Container> {
        let inner = self
            .container
            .upgrade()
            .ok_or_else(|| container_error("container_closed", "容器已关闭"))?;
        if lock(&inner.state).terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        Ok(Container { inner })
    }

    /// 读取实际对象的类型化视图，复用其已有对象身份。
    ///
    /// 尚未构造对象或对象未声明该视图时返回 `None`。视图投影错误原样返回。
    pub fn value<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Option<Arc<T>>> {
        object_view(&self.classes, self.value.as_ref())
    }
}

pub(super) fn object_view<T: ?Sized + Send + Sync + 'static>(
    classes: &ClassRegistry,
    mut cursor: Option<&Value>,
) -> Result<Option<Arc<T>>> {
    let target = ServiceKey::typed::<T>();
    while let Some(value) = cursor {
        if let Some(typed) = value.downcast_ref::<Arc<T>>() {
            return Ok(Some(Arc::clone(typed)));
        }
        if let Some(projection) = classes.output_projection(value.as_ref().type_id(), &target) {
            let projected = projection.project(value)?;
            return projected
                .downcast_ref::<Arc<T>>()
                .cloned()
                .map(Some)
                .ok_or_else(|| {
                    container_error("service_type_mismatch", "解析回调的对象投影类型不匹配")
                });
        }
        cursor = value.origin();
    }
    Ok(None)
}

type ResolveCallback =
    dyn Fn(ResolutionEvent, Resolver) -> BoxFuture<'static, Result<()>> + Send + Sync;
struct Hook(Box<ResolveCallback>);
type HookList = Vec<Arc<Hook>>;

#[derive(Default)]
struct HookGroup {
    global: HookList,
    keyed: Vec<(ServiceKey, HookList)>,
}

impl HookGroup {
    fn register(&mut self, selector: Option<ServiceKey>, hook: Arc<Hook>) {
        let Some(key) = selector else {
            self.global.push(hook);
            return;
        };
        if let Some((_, callbacks)) = self.keyed.iter_mut().find(|(known, _)| *known == key) {
            callbacks.push(hook);
        } else {
            self.keyed.push((key, vec![hook]));
        }
    }

    fn matching(&self, mut matches: impl FnMut(&ServiceKey) -> bool) -> HookList {
        self.keyed
            .iter()
            .filter(|(key, _)| matches(key))
            .flat_map(|(_, callbacks)| callbacks.iter().cloned())
            .collect()
    }

    fn is_empty(&self) -> bool {
        self.global.is_empty() && self.keyed.is_empty()
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Before,
    Resolving,
    After,
}

#[derive(Default)]
pub(super) struct ResolveHooks {
    before: HookGroup,
    resolving: HookGroup,
    after: HookGroup,
}

impl ResolveHooks {
    pub(super) fn dispose(self) -> bool {
        let mut failed = false;
        for group in [self.before, self.resolving, self.after] {
            for hook in group
                .global
                .into_iter()
                .chain(group.keyed.into_iter().flat_map(|(_, hooks)| hooks))
            {
                failed |= disposal::dispose(hook, None).seen;
            }
        }
        failed
    }

    pub(super) fn has_before(&self) -> bool {
        !self.before.is_empty()
    }

    pub(super) fn has_completed(&self) -> bool {
        !self.resolving.is_empty() || !self.after.is_empty()
    }

    fn group(&self, phase: Phase) -> &HookGroup {
        match phase {
            Phase::Before => &self.before,
            Phase::Resolving => &self.resolving,
            Phase::After => &self.after,
        }
    }

    fn register(&mut self, phase: Phase, selector: Option<ServiceKey>, hook: Arc<Hook>) {
        let group = match phase {
            Phase::Before => &mut self.before,
            Phase::Resolving => &mut self.resolving,
            Phase::After => &mut self.after,
        };
        group.register(selector, hook);
    }
}

impl ContainerBuilder {
    /// 在构造或共享缓存读取之前登记解析回调。传入 `None` 登记全局回调。
    pub fn before_resolving(
        &mut self,
        selector: Option<ServiceKey>,
        hook: impl Fn(ResolutionEvent, Resolver) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.register_resolving_hook(Phase::Before, selector, Arc::new(Hook(Box::new(hook))))
    }

    /// 在构造与装饰完成后登记回调。传入 `None` 登记全局回调。
    pub fn resolving(
        &mut self,
        selector: Option<ServiceKey>,
        hook: impl Fn(ResolutionEvent, Resolver) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.register_resolving_hook(Phase::Resolving, selector, Arc::new(Hook(Box::new(hook))))
    }

    /// 在本次构造的所有 `resolving` 回调之后登记回调。传入 `None` 登记全局回调。
    pub fn after_resolving(
        &mut self,
        selector: Option<ServiceKey>,
        hook: impl Fn(ResolutionEvent, Resolver) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.register_resolving_hook(Phase::After, selector, Arc::new(Hook(Box::new(hook))))
    }

    fn register_resolving_hook(
        &mut self,
        phase: Phase,
        selector: Option<ServiceKey>,
        hook: Arc<Hook>,
    ) -> Result<()> {
        let selector = selector
            .map(|key| self.aliases.resolve(&self.classes.canonical_key_owned(key)))
            .transpose()?;
        self.hooks.register(phase, selector, hook);
        Ok(())
    }
}

impl Container {
    /// 在构造或共享缓存读取之前登记解析回调。传入 `None` 登记全局回调。
    pub fn before_resolving(
        &self,
        selector: Option<ServiceKey>,
        hook: impl Fn(ResolutionEvent, Resolver) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.register_resolving_hook(Phase::Before, selector, Arc::new(Hook(Box::new(hook))))
    }

    /// 在构造与装饰完成后登记回调。传入 `None` 登记全局回调。
    pub fn resolving(
        &self,
        selector: Option<ServiceKey>,
        hook: impl Fn(ResolutionEvent, Resolver) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.register_resolving_hook(Phase::Resolving, selector, Arc::new(Hook(Box::new(hook))))
    }

    /// 在本次构造的所有 `resolving` 回调之后登记回调。传入 `None` 登记全局回调。
    pub fn after_resolving(
        &self,
        selector: Option<ServiceKey>,
        hook: impl Fn(ResolutionEvent, Resolver) -> BoxFuture<'static, Result<()>>
        + Send
        + Sync
        + 'static,
    ) -> Result<()> {
        self.register_resolving_hook(Phase::After, selector, Arc::new(Hook(Box::new(hook))))
    }

    fn register_resolving_hook(
        &self,
        phase: Phase,
        selector: Option<ServiceKey>,
        hook: Arc<Hook>,
    ) -> Result<()> {
        let mut state = lock(&self.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        let selector = selector.map(|key| state.alias_key(&key)).transpose()?;
        state.hooks.register(phase, selector, hook);
        Ok(())
    }
}

pub(super) fn object_matches(
    classes: &ClassRegistry,
    mut cursor: Option<&Value>,
    target: &ServiceKey,
) -> bool {
    while let Some(value) = cursor {
        if value.key() == Some(target)
            || classes
                .output_projection(value.as_ref().type_id(), target)
                .is_some()
        {
            return true;
        }
        cursor = value.origin();
    }
    false
}

struct HookOutput {
    value: Option<Value>,
    disposal: CallbackDisposal,
}

impl HookOutput {
    fn new(resolver: &Resolver, value: Value) -> Self {
        Self {
            value: Some(value),
            disposal: resolver.callback_disposal(),
        }
    }

    fn value(&self) -> Result<&Value> {
        self.value
            .as_ref()
            .ok_or_else(|| container_error("service_initialization_failed", "解析回调缺少对象"))
    }

    fn finish(mut self, result: Result<()>) -> Result<Value> {
        let value = self
            .value
            .take()
            .ok_or_else(|| container_error("service_initialization_failed", "解析回调缺少对象"))?;
        match result {
            Ok(()) => Ok(value),
            Err(primary) => self
                .disposal
                .run(|| Err(Resolver::retire_rejected_output(value, primary, false))),
        }
    }
}

impl Drop for HookOutput {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.disposal.run(|| disposal::dispose(value, None));
        }
    }
}

impl Resolver {
    pub(super) async fn fire_before(
        &self,
        inner: &Arc<Inner>,
        key: &ServiceKey,
        arguments: Option<ParameterView>,
    ) -> Result<bool> {
        self.fire_phase(inner, key, None, arguments, Phase::Before)
            .await
    }

    pub(super) async fn fire_completed(
        &self,
        inner: &Arc<Inner>,
        key: &ServiceKey,
        value: &Value,
    ) -> Result<()> {
        self.fire_phase(
            inner,
            key,
            Some(value),
            self.parameters.clone(),
            Phase::Resolving,
        )
        .await?;
        self.fire_phase(
            inner,
            key,
            Some(value),
            self.parameters.clone(),
            Phase::After,
        )
        .await?;
        Ok(())
    }

    pub(super) async fn complete_hook_output(
        &self,
        inner: &Arc<Inner>,
        key: &ServiceKey,
        value: Value,
    ) -> Result<Value> {
        let output = HookOutput::new(self, value);
        let result = self.fire_completed(inner, key, output.value()?).await;
        output.finish(result)
    }

    pub(super) fn observe_published_rebound<'a>(
        &'a self,
        key: &'a ServiceKey,
        generation: u64,
        value: Value,
    ) -> BoxFuture<'a, Result<Value>> {
        let output = HookOutput::new(self, value);
        // 已提交的扩展还包含重绑回调，解析前阶段单独装箱以避免把回调栈带入每个扩展 Future。
        Box::pin(async move {
            let inner = self.container()?.inner;
            let request = ResolveRequest::Erased(key.clone());
            // 已发布值对应 Laravel rebound 的缓存命中，保留本次事件值快照。
            let result = self
                .before_resolution(&inner, &request, Some(generation))
                .await
                .map(drop);
            output.finish(result)
        })
    }

    async fn fire_phase(
        &self,
        inner: &Arc<Inner>,
        key: &ServiceKey,
        value: Option<&Value>,
        arguments: Option<ParameterView>,
        phase: Phase,
    ) -> Result<bool> {
        let (global, classes, termination) = {
            let state = lock(&inner.state);
            self.check_open(&state)?;
            let group = state.hooks.group(phase);
            if group.is_empty() {
                return Ok(false);
            }
            (
                group.global.clone(),
                Arc::clone(&state.classes),
                Arc::clone(&state.termination),
            )
        };
        let event = ResolutionEvent {
            key: key.clone(),
            value: value.cloned(),
            arguments,
            classes,
            container: Arc::downgrade(inner),
        };
        let global_invoked = !global.is_empty();
        for hook in global {
            self.invoke_resolving_hook(inner, &termination, hook, &event)
                .await?;
        }
        // Laravel 在全局组派发完毕后读取类型组。全局回调可登记本阶段的类型回调。
        let keyed = {
            let state = lock(&inner.state);
            self.check_open(&state)?;
            state.hooks.group(phase).matching(|target| {
                target == key
                    || match phase {
                        Phase::Before => state.classes.projects_type(key, target),
                        Phase::Resolving | Phase::After => {
                            object_matches(&state.classes, value, target)
                        }
                    }
            })
        };
        let keyed_invoked = !keyed.is_empty();
        for hook in keyed {
            self.invoke_resolving_hook(inner, &termination, hook, &event)
                .await?;
        }
        Ok(global_invoked || keyed_invoked)
    }

    async fn invoke_resolving_hook(
        &self,
        inner: &Arc<Inner>,
        termination: &super::ScopeStatus,
        hook: Arc<Hook>,
        event: &ResolutionEvent,
    ) -> Result<()> {
        self.check_open(&lock(&inner.state))?;
        let callback = callback_future(
            self.callback_disposal(),
            hook,
            self.clone(),
            Ok(event.clone()),
            None,
            |hook, resolver, event| (hook.0)(event, resolver),
        );
        tokio::select! {
            biased;
            () = termination.wait_closed() => Err(container_error("container_closed", "容器已关闭")),
            () = self.closed.wait_closed() => Err(lock(&inner.state).resolution_closure_error()),
            result = callback => result,
        }?;
        self.check_open(&lock(&inner.state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{class_projection, container::ClassDescriptor};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::sync::Notify;

    trait First: Send + Sync {}
    trait Second: Send + Sync {}
    struct Concrete;
    impl First for Concrete {}
    impl Second for Concrete {}

    #[tokio::test]
    async fn hooks_preserve_selector_groups_and_read_registrations_after_global_callbacks()
    -> Result<()> {
        let mut builder = ContainerBuilder::with_class_descriptors(vec![
            ClassDescriptor::concrete::<Concrete>("App/Concrete", Vec::new(), |_| {
                Ok(Arc::new(Concrete))
            })
            .with_projection(class_projection!(Concrete => dyn First))
            .with_projection(class_projection!(Concrete => dyn Second)),
        ])?;
        let order = Arc::new(Mutex::new(Vec::new()));
        let global_order = Arc::clone(&order);
        builder.resolving(None, move |event, _| {
            let order = Arc::clone(&global_order);
            Box::pin(async move {
                lock(&order).push("global");
                let container = event.container()?;
                container.resolving(Some(ServiceKey::typed::<dyn Second>()), move |event, _| {
                    let order = Arc::clone(&order);
                    Box::pin(async move {
                        assert!(event.value::<dyn Second>()?.is_some());
                        lock(&order).push("second late");
                        Ok(())
                    })
                })
            })
        })?;
        for (selector, label) in [
            (ServiceKey::typed::<dyn First>(), "first one"),
            (ServiceKey::typed::<dyn Second>(), "second"),
            (ServiceKey::typed::<dyn First>(), "first two"),
        ] {
            let order = Arc::clone(&order);
            builder.resolving(Some(selector), move |event, _| {
                let order = Arc::clone(&order);
                Box::pin(async move {
                    assert!(event.value::<Concrete>()?.is_some());
                    lock(&order).push(label);
                    Ok(())
                })
            })?;
        }
        let container = builder.build();
        container.resolve::<Concrete>().await?;
        assert_eq!(
            *lock(&order),
            ["global", "first one", "first two", "second", "second late"]
        );
        Ok(())
    }

    struct ObserveDrop(Arc<AtomicBool>);

    impl Drop for ObserveDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn closing_scope_cancels_pending_resolution_hook() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        builder.bind::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
        let started = Arc::new(Notify::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let notify = Arc::clone(&started);
        let observed_drop = Arc::clone(&dropped);
        builder.resolving(Some(ServiceKey::typed::<usize>()), move |_, _| {
            let guard = ObserveDrop(Arc::clone(&observed_drop));
            notify.notify_one();
            Box::pin(async move {
                let _guard = guard;
                std::future::pending().await
            })
        })?;
        let container = builder.build();
        let scope = container.scope()?;
        let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(scope.resolve::<usize>(), async {
                started.notified().await;
                scope.close();
            })
        })
        .await
        .map_err(|_| container_error("test_timeout", "等待解析回调取消超时"))?;
        assert!(matches!(result, Err(error) if error.code() == "scope_closed"));
        assert!(dropped.load(Ordering::Acquire));
        Ok(())
    }
    #[tokio::test]
    async fn failed_completed_hook_leaves_published_singleton_without_success_history() -> Result<()>
    {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let key = ServiceKey::typed::<u16>();
        let mut builder = ContainerBuilder::new();
        builder.singleton::<u16, _>(|_| Box::pin(async { Ok(Arc::new(17)) }));
        builder.resolving(Some(key.clone()), move |event, resolver| {
            let calls = Arc::clone(&observed);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::Relaxed);
                let value = event
                    .value::<u16>()?
                    .ok_or_else(|| container_error("test_missing_value", "缺少共享对象"))?;
                assert!(Arc::ptr_eq(&value, &resolver.resolve::<u16>().await?));
                assert!(event.container()?.resolved_key(event.key())?);
                Err(container_error("test_hook_failure", "保留回调失败"))
            })
        })?;
        let container = builder.build();
        let error = tokio::time::timeout(Duration::from_secs(3), container.resolve::<u16>())
            .await
            .map_err(|_| container_error("test_hook_timeout", "共享对象读取超时"))?;
        assert!(matches!(error, Err(error) if error.code() == "test_hook_failure"));
        assert!(!lock(&container.inner.state).resolved_history.contains(&key));
        assert_eq!(*container.resolve::<u16>().await?, 17);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        Ok(())
    }
    #[tokio::test]
    async fn singleton_hook_keeps_captive_scope_guard_and_application_resources() -> Result<()> {
        use std::sync::atomic::AtomicUsize;
        let cleaned = Arc::new(AtomicUsize::new(0));
        let cleanup = Arc::clone(&cleaned);
        let mut builder = ContainerBuilder::new();
        builder.singleton::<u16, _>(|_| Box::pin(async { Ok(Arc::new(17)) }));
        builder.scoped::<u32, _>(|_| Box::pin(async { Ok(Arc::new(19)) }));
        builder.after_resolving(Some(ServiceKey::typed::<u16>()), move |_, resolver| {
            let cleanup = Arc::clone(&cleanup);
            Box::pin(async move {
                assert!(matches!(resolver.resolve::<u32>().await, Err(error) if error.code() == "captive_scope_dependency"));
                resolver.resources()?.on_shutdown(move || Box::pin(async move {
                    cleanup.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }))?;
                Ok(())
            })
        })?;
        let container = builder.build();
        let scope = container.scope()?;
        assert_eq!(*scope.resolve::<u16>().await?, 17);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        scope.shutdown(deadline).await?;
        assert_eq!(cleaned.load(Ordering::Relaxed), 0);
        container.shutdown_root(deadline).await?;
        assert_eq!(cleaned.load(Ordering::Relaxed), 1);
        Ok(())
    }
    #[tokio::test]
    async fn before_event_arguments_do_not_replace_ambient_frame_for_direct_factory() -> Result<()>
    {
        use crate::container::{ConstructorParam, ParameterKey, ParameterValue};
        fn arguments(value: &str) -> ParameterOverrides {
            let mut input = ParameterOverrides::new();
            input.insert(
                ParameterKey::name("value"),
                ParameterValue::new(value.to_owned()),
            );
            input
        }
        let mut builder =
            ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<String>(
                "App/Direct",
                vec![ConstructorParam::scalar::<String>("value")],
                |args| args.value(0)?.downcast::<String>(),
            )])?;
        let direct = ServiceKey::typed::<String>();
        builder.bind_class::<String>(direct.clone(), direct.clone());
        builder.bind::<u32, _>(|_| Box::pin(async { Ok(Arc::new(7)) }));
        builder.bind::<u64, _>(|resolver| {
            Box::pin(async move {
                let child = resolver.resolve_with::<u32>(arguments("当前")).await?;
                Ok(Arc::new(u64::from(*child)))
            })
        });
        builder.before_resolving(Some(ServiceKey::typed::<u32>()), move |event, resolver| {
            let direct = direct.clone();
            Box::pin(async move {
                let arguments_view = event.parameters();
                let explicit = arguments_view
                    .get(&ParameterKey::name("value"))
                    .ok_or_else(|| container_error("test_missing_arguments", "缺少本次回调实参"))?
                    .downcast::<String>()?;
                assert_eq!(explicit.as_str(), "当前");
                let raw = event
                    .container()?
                    .get_bindings()?
                    .into_iter()
                    .find(|binding| binding.key == direct)
                    .ok_or_else(|| container_error("test_missing_factory", "缺少直接构造工厂"))?;
                let value = raw
                    .concrete
                    .invoke_with::<String>(resolver, arguments("忽略"))
                    .await?;
                assert_eq!(value.as_str(), "外层");
                Ok(())
            })
        })?;
        let container = builder.build();
        assert_eq!(*container.resolve_with::<u64>(arguments("外层")).await?, 7);
        Ok(())
    }
    #[tokio::test]
    async fn rebound_of_published_instance_runs_before_and_then_listener() -> Result<()> {
        let order = Arc::new(Mutex::new(Vec::new()));
        let before = Arc::clone(&order);
        let after = Arc::clone(&order);
        let rebound = Arc::clone(&order);
        let key = ServiceKey::typed::<u16>();
        let mut builder = ContainerBuilder::new();
        builder.singleton::<u16, _>(|_| Box::pin(async { Ok(Arc::new(17)) }));
        builder.before_resolving(Some(key.clone()), move |_, _| {
            let order = Arc::clone(&before);
            Box::pin(async move {
                lock(&order).push("before");
                Ok(())
            })
        })?;
        builder.after_resolving(Some(key), move |_, _| {
            let order = Arc::clone(&after);
            Box::pin(async move {
                lock(&order).push("after");
                Ok(())
            })
        })?;
        builder.rebinding::<u16, _>(move |_, value| {
            let order = Arc::clone(&rebound);
            Box::pin(async move {
                assert_eq!(*value, 23);
                lock(&order).push("rebound");
                Ok(())
            })
        })?;
        builder.build().instance(Arc::new(23_u16)).await?;
        assert_eq!(*lock(&order), ["before", "rebound"]);
        Ok(())
    }
}

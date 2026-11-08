//! Binding 装饰沿用容器的缓存键、等待图和生命周期，不持内部锁执行用户代码。

use super::{
    ActiveFactory, Arc, AtomicBool, Attempt, Binding, BoxFuture, CacheKey, Container,
    ContainerBuilder, Error, Failure, HashMap, HashSet, Inner, Lifetime, Mutex, Notify, Ordering,
    Resolver, Result, Scope, ScopeStatus, ServiceKey, State, Value, WaitEdge, container_error,
    lock,
};

use super::descriptor_source::{BindingSource, DescriptorSource};
use crate::foundation::resources::{ResourceInlineOwner, ResourceTaskCompletion};

type ExtensionFn = dyn Fn(Value, Resolver) -> BoxFuture<'static, Result<Value>> + Send + Sync;
#[derive(Clone)]
pub(super) struct Extender(pub(super) Arc<ExtensionFn>);
type RebindingFn = dyn Fn(Resolver, Value) -> BoxFuture<'static, Result<()>> + Send + Sync;
#[derive(Clone)]
pub(super) struct Rebinding(Arc<RebindingFn>);

// 一个已提交事件只交付该次值；后继目录和回调登记不会改变此快照。
pub(super) struct Rebound {
    resolver: Resolver,
    service: ServiceKey,
    generation: u64,
    callbacks: Vec<Rebinding>,
    value: Option<Value>,
}

impl Rebound {
    pub(super) fn capture(
        state: &State,
        inner: &Inner,
        caller: &Resolver,
        service: &ServiceKey,
        binding: &Binding,
        value: Option<Value>,
    ) -> Option<Self> {
        let callbacks = state.extensions.listeners.get(service)?;
        if callbacks.is_empty() {
            return None;
        }
        let mut resolver = caller.clone();
        resolver.chain.clear();
        resolver.constructing = None;
        if state.service_lifetime(service, Some(binding)) == Lifetime::Singleton {
            resolver.captive = true;
            resolver.scope = u64::MAX;
            resolver.resources.clone_from(&inner.resources);
        }
        Some(Self {
            resolver,
            service: service.clone(),
            generation: binding.generation,
            callbacks: callbacks.clone(),
            value,
        })
    }

    pub(super) async fn dispatch(self) -> Result<()> {
        let value = match self.value {
            Some(value) => {
                self.resolver
                    .observe_published_rebound(&self.service, self.generation, value)
                    .await?
            }
            None => {
                self.resolver
                    .resolve_at_generation(self.service, Some(self.generation))
                    .await?
            }
        };
        for callback in self.callbacks {
            tokio::select! {
                result = (callback.0)(self.resolver.clone(), Arc::clone(&value)) => result?,
                () = self.resolver.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
            }
        }
        Ok(())
    }
}

tokio::task_local! {
    // 回调在发布后允许同型 resolve，但仍不得递归 extend 自己。
    static EXTENDING: Vec<(usize, CacheKey, u64)>;
}

// raw Closure 的受管执行仍携带本容器的装饰上下文，避免跨 task 丢失递归边界。
pub(super) fn extension_context() -> Vec<(usize, CacheKey, u64)> {
    EXTENDING.try_with(Clone::clone).unwrap_or_default()
}

pub(super) async fn with_extension_context<F: std::future::Future>(
    stack: Vec<(usize, CacheKey, u64)>,
    future: F,
) -> F::Output {
    EXTENDING.scope(stack, future).await
}

enum TransientSource {
    Binding(u64),
    Contextual,
    Descriptor {
        revision: u64,
        completion: ResourceTaskCompletion,
    },
}

struct TransientAttempt {
    source: TransientSource,
    attempt: Arc<Attempt>,
}

pub(super) struct CachedAdmission {
    pub(super) attempt: Arc<Attempt>,
    // None 表示跟随已有 owner；空向量仍是有权构造的 owner。
    pub(super) owner_chain: Option<Vec<Extender>>,
    pub(super) edge: Option<WaitEdge>,
}

#[derive(Default)]
pub(super) struct ExtensionState {
    global: HashMap<ServiceKey, Vec<Extender>>,
    local: HashMap<CacheKey, Vec<Extender>>,
    listeners: HashMap<ServiceKey, Vec<Rebinding>>,
    pub(super) resolved: HashSet<CacheKey>,
    pending_cursor: HashMap<ServiceKey, usize>,
    pending_order: Vec<(ServiceKey, usize)>,
    operations: HashMap<CacheKey, Arc<Operation>>,
    transients: HashMap<CacheKey, Vec<TransientAttempt>>,
}

impl ExtensionState {
    pub(super) fn take_listeners(&mut self) -> HashMap<ServiceKey, Vec<Rebinding>> {
        std::mem::take(&mut self.listeners)
    }

    pub(super) fn flush_resolved(&mut self) -> HashMap<CacheKey, Arc<Operation>> {
        self.resolved.clear();
        self.pending_cursor.clear();
        self.pending_order.clear();
        // 在途构造的完成票据继续由原调用退役；只撤销旧装饰操作的发布资格。
        std::mem::take(&mut self.operations)
    }

    pub(super) fn discard_operations(&mut self, service: &ServiceKey) -> Vec<Arc<Operation>> {
        let keys: Vec<_> = self
            .operations
            .keys()
            .filter(|key| &key.service == service)
            .cloned()
            .collect();
        keys.into_iter()
            .filter_map(|key| self.operations.remove(&key))
            .collect()
    }
    pub(super) fn discard_pending(&mut self, service: &ServiceKey) {
        self.pending_order
            .retain(|(registered, _)| registered != service);
        self.pending_cursor.remove(service);
    }

    pub(super) fn instance_complete(&self, service: &ServiceKey, total: usize) -> bool {
        self.pending_cursor.get(service).copied().unwrap_or(0) >= total
    }

    pub(super) fn pending_operation(&self, key: &CacheKey) -> bool {
        self.operations
            .get(key)
            .is_some_and(|operation| !operation.published.load(Ordering::Acquire))
    }

    pub(super) fn discard_scope(&mut self, scope: u64) -> Vec<Extender> {
        self.resolved.retain(|key| key.scope != scope);
        let keys: Vec<_> = self
            .local
            .keys()
            .filter(|key| key.scope == scope)
            .cloned()
            .collect();
        keys.into_iter()
            .filter_map(|key| self.local.remove(&key))
            .flatten()
            .collect()
    }

    pub(super) fn descriptor_completions(
        &self,
    ) -> impl Iterator<Item = ResourceTaskCompletion> + '_ {
        self.transients
            .values()
            .flatten()
            .filter_map(|record| match &record.source {
                TransientSource::Binding(_) | TransientSource::Contextual => None,
                TransientSource::Descriptor { completion, .. } => Some(completion.clone()),
            })
    }

    pub(super) fn chain(&self, key: &CacheKey) -> Vec<Extender> {
        self.global
            .get(&key.service)
            .into_iter()
            .flatten()
            .chain(self.local.get(key).into_iter().flatten())
            .cloned()
            .collect()
    }
}

struct ExtenderProvenance {
    previous: Option<Value>,
    disposal: crate::foundation::lifecycle::CallbackDisposal,
}

impl ExtenderProvenance {
    fn finish(mut self, result: Result<Value>) -> Result<Value> {
        use crate::foundation::lifecycle::{TaskExecutionFailure, disposal};
        let Some(previous) = self.previous.take() else {
            return result;
        };
        self.disposal.run(|| {
            let result = result.map(|next| super::resolved_value::after_extension(&previous, next));
            if !disposal::dispose(previous, None).seen {
                return result;
            }
            let cleanup = container_error("resource_task_interrupted", "旧投影视图清理异常");
            match result {
                Ok(value) => {
                    disposal::dispose(value, None);
                    Err(cleanup)
                }
                Err(primary) => Err(TaskExecutionFailure::error(primary, cleanup)),
            }
        })
    }
}

impl Drop for ExtenderProvenance {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            self.disposal
                .run(|| crate::foundation::lifecycle::disposal::dispose(previous, None));
        }
    }
}

fn erase_extender<T, F>(extender: F) -> Extender
where
    T: ?Sized + Send + Sync + 'static,
    F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
{
    let extender = Arc::new(extender);
    Extender(Arc::new(move |value, resolver| {
        let typed = value
            .downcast_ref::<Arc<T>>()
            .cloned()
            .ok_or_else(|| container_error("service_type_mismatch", "装饰器输入类型不匹配"));
        let disposal = resolver.callback_disposal();
        let provenance = ExtenderProvenance {
            previous: value.origin().map(|_| Arc::clone(&value)),
            disposal: disposal.clone(),
        };
        // 直接交用户 Future 给守卫。额外 async 层只包已隔离的轮询结果。
        let guarded = crate::foundation::lifecycle::callback_future(
            disposal,
            Arc::clone(&extender),
            resolver,
            typed,
            Some(value),
            |extender, resolver, value| extender(value, resolver),
        );
        Box::pin(async move { provenance.finish(guarded.await.map(super::erase)) })
    }))
}
fn erase_rebinding<T, F>(callback: F) -> Rebinding
where
    T: ?Sized + Send + Sync + 'static,
    F: Fn(Resolver, Arc<T>) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
{
    let callback = Arc::new(callback);
    Rebinding(Arc::new(move |resolver, value| {
        let typed = value
            .downcast_ref::<Arc<T>>()
            .cloned()
            .ok_or_else(|| container_error("service_type_mismatch", "重绑定通知类型不匹配"));
        Box::pin(crate::foundation::lifecycle::callback_future(
            resolver.callback_disposal(),
            Arc::clone(&callback),
            resolver,
            typed,
            Some(value),
            |callback, resolver, value| callback(resolver, value),
        ))
    }))
}

impl ContainerBuilder {
    /// 装饰原 factory；显式 instance 的异步变换延至 Provider boot 前或首次解析。
    ///
    /// 未绑定类型也能登记；factory 覆盖保留持久链，instance 覆盖丢弃旧即时链。
    ///
    /// # Errors
    ///
    /// 规范原键的别名链成环时返回 `service_alias_cycle`，不登记装饰器。
    pub fn extend<T, F>(&mut self, extender: F) -> Result<&mut Self>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.extend_key(&ServiceKey::typed::<T>(), extender)
    }

    /// 沿别名链装饰服务。回调类型必须匹配服务的声明输出。
    pub fn extend_key<T, F>(&mut self, key: &ServiceKey, extender: F) -> Result<&mut Self>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        let key = self.aliases.resolve(&self.classes.canonical_key(key))?;
        let extender = erase_extender::<T, F>(extender);
        if let Some(binding) = self
            .bindings
            .get_mut(&key)
            .filter(|binding| binding.instance.is_some())
        {
            binding.pending.push(extender);
            self.extensions
                .pending_order
                .push((key.clone(), binding.pending.len()));
        } else {
            self.extensions
                .global
                .entry(key)
                .or_default()
                .push(extender);
        }
        Ok(self)
    }

    /// 清除持久装饰链，不撤销当前 instance 或其待执行即时装饰。
    ///
    /// # Errors
    ///
    /// 别名链成环时返回 `service_alias_cycle`，不移除任何链。
    pub fn forget_extenders<T: ?Sized + Send + Sync + 'static>(&mut self) -> Result<&mut Self> {
        self.forget_extenders_key(&ServiceKey::typed::<T>())
    }

    /// 沿别名链清除未来解析使用的持久装饰链。
    pub fn forget_extenders_key(&mut self, key: &ServiceKey) -> Result<&mut Self> {
        let key = self.aliases.resolve(&self.classes.canonical_key(key))?;
        self.extensions.global.remove(&key);
        Ok(self)
    }

    /// 登记装饰成功后的异步通知；新缓存发布后按登记顺序执行。
    ///
    /// # Errors
    ///
    /// 别名链成环时返回 `service_alias_cycle`，不登记监听器。
    pub fn rebinding<T, F>(&mut self, callback: F) -> Result<&mut Self>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver, Arc<T>) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    {
        self.rebinding_key(&ServiceKey::typed::<T>(), callback)
    }

    /// 沿别名链登记重绑定通知。回调类型必须匹配服务的声明输出。
    pub fn rebinding_key<T, F>(&mut self, key: &ServiceKey, callback: F) -> Result<&mut Self>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver, Arc<T>) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    {
        let key = self.aliases.resolve(&self.classes.canonical_key(key))?;
        self.extensions
            .listeners
            .entry(key)
            .or_default()
            .push(erase_rebinding::<T, F>(callback));
        Ok(self)
    }
}

pub(super) struct Operation {
    attempt: Arc<Attempt>,
    published: AtomicBool,
}

impl Operation {
    pub(super) fn attempt_id(&self) -> u64 {
        self.attempt.id
    }

    pub(super) fn release(&self) {
        let mut outcome = lock(&self.attempt.outcome);
        if outcome.is_none() {
            *outcome = Some(Err(Failure {
                code: "binding_replaced",
                message: "服务绑定已更换；等待操作重新准入".into(),
                source: None,
                unresolvable: false,
            }));
        }
        drop(outcome);
        self.published.store(true, Ordering::Release);
        self.attempt.changed.notify_waiters();
    }
    async fn visible(&self) {
        loop {
            let notified = self.attempt.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.published.load(Ordering::Acquire) || lock(&self.attempt.outcome).is_some() {
                return;
            }
            notified.await;
        }
    }
}

struct OperationGuard {
    inner: Arc<Inner>,
    key: CacheKey,
    operation: Arc<Operation>,
    generation: u64,
    owner: Resolver,
    local: bool,
}

impl OperationGuard {
    fn check(&self, state: &State) -> Result<()> {
        self.owner.check_open(state)?;
        if state.bindings.get(&self.key.service).map_or_else(
            || state.class_revision(&self.key.service),
            |binding| binding.generation,
        ) == self.generation
            && state
                .extensions
                .operations
                .get(&self.key)
                .is_some_and(|current| Arc::ptr_eq(current, &self.operation))
        {
            return Ok(());
        }
        Err(container_error(
            "binding_replaced",
            "装饰期间服务绑定已更换",
        ))
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        let removed = {
            let mut state = lock(&self.inner.state);
            state
                .edges
                .retain(|(from, _), _| *from != self.operation.attempt.id);
            if state
                .extensions
                .operations
                .get(&self.key)
                .is_some_and(|op| Arc::ptr_eq(op, &self.operation))
            {
                state.extensions.operations.remove(&self.key)
            } else {
                None
            }
        };
        let mut outcome = lock(&self.operation.attempt.outcome);
        if outcome.is_none() {
            *outcome = Some(Ok(super::plain(())));
        }
        drop(outcome);
        self.operation.attempt.changed.notify_waiters();
        drop(removed);
    }
}

fn address(inner: &Arc<Inner>) -> usize {
    Arc::as_ptr(inner) as usize
}

fn extension_parent(inner: &Arc<Inner>) -> Option<u64> {
    EXTENDING
        .try_with(|stack| {
            stack
                .iter()
                .rev()
                .find(|(owner, _, _)| *owner == address(inner))
                .map(|(_, _, id)| *id)
        })
        .ok()
        .flatten()
}

fn recursive_extension(inner: &Arc<Inner>, key: &CacheKey) -> bool {
    EXTENDING
        .try_with(|stack| {
            stack
                .iter()
                .any(|(owner, active, _)| *owner == address(inner) && active == key)
        })
        .unwrap_or(false)
}

impl Resolver {
    pub(super) fn check_open(&self, state: &State) -> Result<()> {
        if state.terminated || self.closed.closed.load(Ordering::Acquire) {
            return Err(state.resolution_closure_error());
        }
        Ok(())
    }

    pub(super) fn wait_edge(
        &self,
        inner: &Arc<Inner>,
        state: &mut State,
        attempt: &Attempt,
    ) -> Result<Option<WaitEdge>> {
        let parent = self
            .constructing
            .as_ref()
            .filter(|(_, active)| active.load(Ordering::Acquire))
            .map(|(id, _)| *id)
            .or_else(|| extension_parent(inner));
        let Some(parent) = parent else {
            return Ok(None);
        };
        if lock(&attempt.outcome).is_some() {
            return Ok(None);
        }
        if let Some(mut path) = state.path(attempt.id, parent) {
            path.insert(0, parent);
            let names = path
                .into_iter()
                .filter_map(|id| {
                    let attempts = state.slots.values().map(Arc::as_ref);
                    #[cfg(test)]
                    let attempts = attempts.chain(
                        state
                            .raw_attempts
                            .values()
                            .map(|record| record.attempt.as_ref()),
                    );
                    attempts
                        .chain(
                            state
                                .binding_discoveries
                                .values()
                                .map(|flight| &flight.attempt),
                        )
                        .chain(
                            state
                                .extensions
                                .operations
                                .values()
                                .map(|operation| operation.attempt.as_ref()),
                        )
                        .chain(
                            state
                                .extensions
                                .transients
                                .values()
                                .flatten()
                                .map(|record| record.attempt.as_ref()),
                        )
                        .find(|slot| slot.id == id)
                        .map(|slot| slot.service.to_string())
                })
                .collect::<Vec<_>>();
            return Err(container_error("service_cycle", names.join(" → ")));
        }
        *state.edges.entry((parent, attempt.id)).or_default() += 1;
        Ok(Some(WaitEdge {
            inner: Arc::clone(inner),
            edge: (parent, attempt.id),
        }))
    }

    async fn wait_visible(&self, operation: &Operation) -> Result<()> {
        tokio::select! {
            () = operation.visible() => Ok(()),
            () = self.closed.wait_closed() => Err(container_error("scope_closed", "生命周期已关闭")),
        }
    }

    pub(super) async fn admit_cached(
        &self,
        inner: &Arc<Inner>,
        key: &CacheKey,
        source: &BindingSource,
    ) -> Result<Option<CachedAdmission>> {
        loop {
            let waiting = {
                let mut state = lock(&inner.state);
                self.check_open(&state)?;
                if !source.is_current(&state) {
                    return Ok(None);
                }
                if let Some(operation) = state
                    .extensions
                    .operations
                    .get(key)
                    .filter(|op| !op.published.load(Ordering::Acquire))
                    .cloned()
                {
                    let edge = self.wait_edge(inner, &mut state, &operation.attempt)?;
                    Some((operation, edge))
                } else {
                    let (attempt, owner_chain) = if let Some(attempt) = state.slots.get(key) {
                        (Arc::clone(attempt), None)
                    } else {
                        let attempt = Arc::new(Attempt {
                            id: state.id()?,
                            service: key.service.clone(),
                            outcome: Mutex::new(None),
                            changed: Notify::new(),
                        });
                        state.slots.insert(key.clone(), Arc::clone(&attempt));
                        let chain = if source.binding.instance.is_some() {
                            Vec::new()
                        } else {
                            state.extensions.chain(key)
                        };
                        (attempt, Some(chain))
                    };
                    let edge = self.wait_edge(inner, &mut state, &attempt)?;
                    return Ok(Some(CachedAdmission {
                        attempt,
                        owner_chain,
                        edge,
                    }));
                }
            };
            if let Some((operation, _edge)) = waiting {
                self.wait_visible(&operation).await?;
            }
        }
    }

    pub(super) async fn construct_value(
        &self,
        binding: &Binding,
        chain: Vec<Extender>,
    ) -> Result<Value> {
        #[cfg(test)]
        super::binding_admission_tests::after_owner_admission().await;
        let mut value = if let Some(instance) = &binding.instance {
            Arc::clone(instance)
        } else {
            let factory = binding
                .factory
                .as_ref()
                .ok_or_else(|| container_error("service_not_bound", "服务没有工厂或实例"))?;
            let mut closure = self.clone();
            closure.build_consumer = None;
            factory(closure).await?
        };
        for extender in chain {
            let mut resolver = self.clone();
            if let Some((service, _)) = self.chain.last() {
                resolver.suppress_self_building_reentry(service);
            }
            value = (extender.0)(value, resolver).await?;
        }
        Ok(value)
    }

    pub(super) async fn transient(
        &self,
        source: &BindingSource,
        mut child: Self,
        activity: Arc<AtomicBool>,
        events: bool,
    ) -> Result<Option<Value>> {
        let inner = self.container()?.inner;
        let service = &source.key;
        let binding = &source.binding;
        let key = CacheKey {
            service: service.clone(),
            scope: self.scope,
        };
        let (attempt, chain, _edge) = loop {
            let waiting = {
                let mut state = lock(&inner.state);
                self.check_open(&state)?;
                if !source.is_current(&state) {
                    return Ok(None);
                }
                if let Some(operation) = state
                    .extensions
                    .operations
                    .get(&key)
                    .filter(|op| !op.published.load(Ordering::Acquire))
                    .cloned()
                {
                    let edge = self.wait_edge(&inner, &mut state, &operation.attempt)?;
                    Some((operation, edge))
                } else {
                    let attempt = Arc::new(Attempt {
                        id: state.id()?,
                        service: key.service.clone(),
                        outcome: Mutex::new(None),
                        changed: Notify::new(),
                    });
                    state
                        .extensions
                        .transients
                        .entry(key.clone())
                        .or_default()
                        .push(TransientAttempt {
                            source: if source.contextual.is_some() {
                                TransientSource::Contextual
                            } else {
                                TransientSource::Binding(binding.generation)
                            },
                            attempt: Arc::clone(&attempt),
                        });
                    let edge = self.wait_edge(&inner, &mut state, &attempt)?;
                    break (attempt, state.extensions.chain(&key), edge);
                }
            };
            if let Some((operation, _edge)) = waiting {
                self.wait_visible(&operation).await?;
            }
        };
        let mut guard = TransientGuard {
            inner: Arc::clone(&inner),
            key: key.clone(),
            attempt: Arc::clone(&attempt),
            generation: binding.generation,
            activity,
            closed: Arc::clone(&self.closed),
            record_history: self.parameters.is_none() && source.contextual.is_none(),
            finished: false,
        };
        child.constructing = Some((attempt.id, Arc::clone(&guard.activity)));
        let work = async {
            let value = child.construct_value(binding, chain).await?;
            if events {
                child.suppress_self_building_reentry(service);
                child.complete_hook_output(&inner, service, value).await
            } else {
                Ok(value)
            }
        };
        let result = tokio::select! {
            result = work => result,
            () = self.closed.wait_closed() => Err(lock(&inner.state).resolution_closure_error()),
        };
        guard.finish(result.as_ref().map(Arc::clone).map_err(|error| {
            Failure::initialization(service, Error::custom(error.code(), error.to_string()))
        }));
        result.map(Some)
    }
}

impl ExtensionState {
    fn retire_transient(&mut self, key: &CacheKey, id: u64) -> Option<TransientAttempt> {
        let records = self.transients.get_mut(key)?;
        let removed = records
            .iter()
            .position(|record| record.attempt.id == id)
            .map(|position| records.remove(position));
        if records.is_empty() {
            self.transients.remove(key);
        }
        removed
    }
}

pub(super) struct DescriptorAttempt {
    inner: Arc<Inner>,
    pub(super) key: CacheKey,
    attempt: Arc<Attempt>,
    edge: Option<WaitEdge>,
    activity: Arc<AtomicBool>,
    pub(super) termination: Arc<ScopeStatus>,
    owner: Option<ResourceInlineOwner>,
}

impl DescriptorAttempt {
    pub(super) fn admit(
        inner: &Arc<Inner>,
        resolver: &Resolver,
        source: &DescriptorSource,
    ) -> Result<Option<Self>> {
        let mut owner = ResourceInlineOwner::new();
        let completion = owner.completion();
        let key = CacheKey {
            service: source.key().clone(),
            scope: resolver.scope,
        };
        let reserved = {
            let mut state = lock(&inner.state);
            if !source.same_scope(&state, resolver)? {
                return Ok(None);
            }
            if !source.is_current(&state) {
                source.owner.with_open_owner(|| ())?;
                return Ok(None);
            }
            owner.reserve(&source.owner, || {
                let attempt = Arc::new(Attempt {
                    id: state.id()?,
                    service: key.service.clone(),
                    outcome: Mutex::new(None),
                    changed: Notify::new(),
                });
                let edge = resolver.wait_edge(inner, &mut state, &attempt)?;
                state
                    .extensions
                    .transients
                    .entry(key.clone())
                    .or_default()
                    .push(TransientAttempt {
                        source: TransientSource::Descriptor {
                            revision: source.revision,
                            completion,
                        },
                        attempt: Arc::clone(&attempt),
                    });
                Ok((attempt, edge, Arc::clone(&state.termination)))
            })?
        };
        let (attempt, edge, termination) = reserved;
        let mut admitted = Self {
            inner: Arc::clone(inner),
            key,
            attempt,
            edge,
            termination,
            activity: Arc::new(AtomicBool::new(true)),
            owner: Some(owner),
        };
        if let Some(owner) = &mut admitted.owner {
            owner.register()?;
        }
        Ok(Some(admitted))
    }

    pub(super) fn owner(&self) -> Result<&ResourceInlineOwner> {
        self.owner
            .as_ref()
            .ok_or_else(|| container_error("resource_inline_state_invalid", "描述符构造已退役"))
    }

    pub(super) fn child(&self, resolver: &Resolver) -> Result<Resolver> {
        let mut child = resolver.clone();
        child
            .chain
            .retain(|(_, active)| active.load(Ordering::Acquire));
        child
            .chain
            .push((self.key.service.clone(), Arc::clone(&self.activity)));
        child.constructing = Some((self.attempt.id, Arc::clone(&self.activity)));
        child.factory_completion = Some(self.owner()?.completion());
        Ok(child)
    }

    pub(super) fn active(&self, state: &State, source: &DescriptorSource) -> bool {
        state.extensions.transients.get(&self.key).is_some_and(|records| records.iter().any(|record| {
            record.attempt.id == self.attempt.id &&
                matches!(record.source, TransientSource::Descriptor { revision, .. } if revision == source.revision)
        }))
    }
}

impl Drop for DescriptorAttempt {
    fn drop(&mut self) {
        self.activity.store(false, Ordering::Release);
        let removed = {
            let mut state = lock(&self.inner.state);
            state
                .edges
                .retain(|(from, to), _| *from != self.attempt.id && *to != self.attempt.id);
            state
                .extensions
                .retire_transient(&self.key, self.attempt.id)
        };
        drop(removed);
        drop(self.edge.take());
        self.attempt.changed.notify_waiters();
        if let Some(owner) = self.owner.take() {
            owner.finish();
        }
    }
}

struct TransientGuard {
    inner: Arc<Inner>,
    key: CacheKey,
    attempt: Arc<Attempt>,
    generation: u64,
    activity: Arc<AtomicBool>,
    closed: Arc<ScopeStatus>,
    record_history: bool,
    finished: bool,
}

impl TransientGuard {
    fn finish(&mut self, result: std::result::Result<Value, Failure>) {
        self.activity.store(false, Ordering::Release);
        let mut unclaimed = Some(result);
        let removed = {
            let mut state = lock(&self.inner.state);
            let mut outcome = lock(&self.attempt.outcome);
            if outcome.is_none() {
                *outcome = unclaimed.take();
                if self.record_history
                    && outcome.as_ref().is_some_and(std::result::Result::is_ok)
                    && !state.terminated
                    && !self.closed.closed.load(Ordering::Acquire)
                    && state
                        .bindings
                        .get(&self.key.service)
                        .map(|binding| binding.generation)
                        == Some(self.generation)
                {
                    state.record_success(&self.key);
                }
            }
            state.edges.retain(|(from, _), _| *from != self.attempt.id);
            // 保留 guard 的 Arc，确保最后一个用户 Value 不会在内部锁内释放。
            state
                .extensions
                .retire_transient(&self.key, self.attempt.id)
        };
        drop(unclaimed);
        self.finished = true;
        self.attempt.changed.notify_waiters();
        drop(removed);
    }
}

impl Drop for TransientGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(Err(Failure {
                code: "service_initialization_cancelled",
                message: "服务构造已取消".into(),
                source: None,
                unresolvable: false,
            }));
        }
    }
}

impl Container {
    /// 登记异步重绑定通知；已绑定服务立即解析，后续重绑按登记顺序通知。
    pub async fn rebinding<T, F>(&self, callback: F) -> Result<Option<Arc<T>>>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver, Arc<T>) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    {
        self.rebinding_key(ServiceKey::typed::<T>(), callback).await
    }

    /// 沿别名链登记重绑定通知，已绑定时立即解析当前值。
    /// 回调类型必须匹配服务的声明输出。
    pub async fn rebinding_key<T, F>(&self, key: ServiceKey, callback: F) -> Result<Option<Arc<T>>>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver, Arc<T>) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    {
        let (key, bound) = {
            let mut state = lock(&self.inner.state);
            if state.terminated {
                return Err(container_error("container_closed", "容器已关闭"));
            }
            let key = state.alias_key(&key)?;
            state
                .extensions
                .listeners
                .entry(key.clone())
                .or_default()
                .push(erase_rebinding::<T, F>(callback));
            let bound = state.bound_key(&key);
            (key, bound)
        };
        if bound {
            self.root_resolver()?.resolve_key::<T>(key).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// 装饰当前共享缓存；未缓存时保留持久链供未来构造，现有兄弟 scope 不广播替换。
    ///
    /// 失败或取消不替换旧缓存；已发布后的 rebinding 错误不会回滚新值。
    pub async fn extend<T, F>(&self, extender: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.extend_key(&ServiceKey::typed::<T>(), extender).await
    }

    /// 沿别名链装饰根服务。回调类型必须匹配服务的声明输出。
    pub async fn extend_key<T, F>(&self, key: &ServiceKey, extender: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.root_resolver()?
            .extend_value(key, erase_extender::<T, F>(extender), false)
            .await
    }

    /// 清除未来构造的持久链；不撤销当前缓存或 instance 的即时变换。
    pub async fn forget_extenders<T: ?Sized + Send + Sync + 'static>(&self) -> Result<()> {
        self.forget_extenders_key(&ServiceKey::typed::<T>()).await
    }

    /// 沿别名链清除根服务的持久装饰链。
    pub async fn forget_extenders_key(&self, key: &ServiceKey) -> Result<()> {
        self.root_resolver()?.forget_chain(key, false).await
    }

    pub(crate) async fn initialize_extensions(&self) -> Result<()> {
        // 只保存实例即时操作的登记顺序，不让 HashMap 随机遍历改变跨类型副作用。
        let pending = lock(&self.inner.state).extensions.pending_order.clone();
        for (service, through) in pending {
            let binding = lock(&self.inner.state).bindings.get(&service).cloned();
            if let Some(binding) = binding {
                self.root_resolver()?
                    .initialize_instance_until(
                        &self.inner,
                        &BindingSource::pinned(service, binding),
                        through,
                    )
                    .await?;
            }
        }
        Ok(())
    }
}

impl Scope {
    /// 仅变换本 scope 当前缓存或未来构造；singleton 仍归应用，不影响兄弟 scoped 缓存。
    pub async fn extend<T, F>(&self, extender: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.extend_key(&ServiceKey::typed::<T>(), extender).await
    }

    /// 沿别名链装饰本作用域的服务。回调类型必须匹配服务的声明输出。
    pub async fn extend_key<T, F>(&self, key: &ServiceKey, extender: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.resolver()
            .extend_value(key, erase_extender::<T, F>(extender), true)
            .await
    }

    /// 仅清除本 scope 的持久装饰；singleton 的持久链由应用清除。
    pub async fn forget_extenders<T: ?Sized + Send + Sync + 'static>(&self) -> Result<()> {
        self.forget_extenders_key(&ServiceKey::typed::<T>()).await
    }

    /// 沿别名链清除本作用域的持久装饰链。
    pub async fn forget_extenders_key(&self, key: &ServiceKey) -> Result<()> {
        self.resolver().forget_chain(key, true).await
    }
}

impl Resolver {
    async fn operation(
        &self,
        inner: &Arc<Inner>,
        service: &ServiceKey,
        local: bool,
        expected: Option<&BindingSource>,
    ) -> Result<OperationGuard> {
        loop {
            let waiting = {
                let mut state = lock(&inner.state);
                self.check_open(&state)?;
                let binding = state.bindings.get(service);
                let generation = binding.map_or_else(
                    || state.class_revision(service),
                    |binding| binding.generation,
                );
                if expected.is_some_and(|source| !source.is_current(&state)) {
                    return Err(container_error(
                        "binding_replaced",
                        "装饰准入期间服务绑定已更换",
                    ));
                }
                let singleton = state.service_lifetime(service, binding) == Lifetime::Singleton;
                let key = CacheKey {
                    service: service.clone(),
                    scope: if singleton { u64::MAX } else { self.scope },
                };
                if recursive_extension(inner, &key) {
                    return Err(container_error(
                        "service_cycle",
                        format!("服务 `{service}` 不能递归装饰自己"),
                    ));
                }
                if let Some(operation) = state.extensions.operations.get(&key).cloned() {
                    let edge = self.wait_edge(inner, &mut state, &operation.attempt)?;
                    Some((operation, edge))
                } else {
                    let operation = Arc::new(Operation {
                        attempt: Arc::new(Attempt {
                            id: state.id()?,
                            service: key.service.clone(),
                            outcome: Mutex::new(None),
                            changed: Notify::new(),
                        }),
                        published: AtomicBool::new(false),
                    });
                    state
                        .extensions
                        .operations
                        .insert(key.clone(), Arc::clone(&operation));
                    let owner = if singleton && local {
                        Container {
                            inner: Arc::clone(inner),
                        }
                        .resolver(
                            state.root_scope,
                            Arc::clone(&state.root_closed),
                            state.root_resources.registrar(),
                        )
                    } else {
                        self.clone()
                    };
                    return Ok(OperationGuard {
                        inner: Arc::clone(inner),
                        key,
                        owner,
                        local: local && !singleton,
                        operation,
                        generation,
                    });
                }
            };
            if let Some((operation, _edge)) = waiting {
                tokio::select! {
                    _ = operation.attempt.wait() => {},
                    () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
                }
            }
        }
    }

    async fn wait_constructions(&self, guard: &OperationGuard) -> Result<()> {
        let waiting = {
            let mut state = lock(&guard.inner.state);
            guard.check(&state)?;
            let mut attempts = state
                .slots
                .get(&guard.key)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            attempts.extend(
                state
                    .extensions
                    .transients
                    .get(&guard.key)
                    .into_iter()
                    .flatten()
                    .filter(|record| matches!(record.source, TransientSource::Binding(generation) if generation == guard.generation))
                    .map(|record| Arc::clone(&record.attempt)),
            );
            let mut waiting = Vec::new();
            let mut resolver = self.clone();
            resolver.constructing =
                Some((guard.operation.attempt.id, Arc::new(AtomicBool::new(true))));
            for attempt in attempts {
                let edge = match resolver.wait_edge(&guard.inner, &mut state, &attempt) {
                    Ok(edge) => edge,
                    Err(error) => {
                        // 已登记的 WaitEdge 析构也会取 state 锁，错误退出必须先释放锁。
                        drop(state);
                        drop(waiting);
                        return Err(error);
                    }
                };
                waiting.push((attempt, edge));
            }
            waiting
        };
        for (attempt, _edge) in waiting {
            tokio::select! {
                // 原构造失败后回到无缓存分支；它不属于本次装饰器失败。
                _ = attempt.wait() => {},
                _ = guard.operation.attempt.wait() => return Err(container_error("binding_replaced", "等待构造期间服务绑定已更换")),
                () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
            }
        }
        Ok(())
    }

    fn extension_resolver(&self, guard: &OperationGuard) -> (Self, ActiveFactory) {
        let mut child = self.clone();
        child
            .chain
            .retain(|(_, active)| active.load(Ordering::Acquire));
        let active = Arc::new(AtomicBool::new(true));
        child
            .chain
            .push((guard.key.service.clone(), Arc::clone(&active)));
        child.constructing = Some((guard.operation.attempt.id, Arc::clone(&active)));
        if guard.key.scope == u64::MAX {
            child.captive = true;
            child.scope = u64::MAX;
            child.resources.clone_from(&guard.inner.resources);
        }
        (child, ActiveFactory(active))
    }

    pub(super) async fn extend_value(
        &self,
        service: &ServiceKey,
        extender: Extender,
        local: bool,
    ) -> Result<()> {
        let inner = self.container()?.inner;
        let service = lock(&inner.state).alias_key(service)?;
        let service = &service;
        let binding = lock(&inner.state).bindings.get(service).cloned();
        if let Some(binding) = binding.filter(|binding| !binding.pending.is_empty()) {
            self.initialize_instance(&inner, &BindingSource::pinned(service.clone(), binding))
                .await?;
        }
        let guard = self.operation(&inner, service, local, None).await?;
        let owner = &guard.owner;
        let key = &guard.key;
        let mut stack = EXTENDING.try_with(Clone::clone).unwrap_or_default();
        stack.push((address(&inner), key.clone(), guard.operation.attempt.id));
        EXTENDING.scope(stack, async {
            owner.wait_constructions(&guard).await?;
            let old = {
                let state = lock(&inner.state);
                guard.check(&state)?;
                state.slots.get(key).and_then(|slot| lock(&slot.outcome).as_ref().and_then(|value| value.as_ref().ok().cloned()))
                    .or_else(|| state.bindings.get(service).filter(|binding| binding.pending.is_empty()).and_then(|binding| binding.instance.clone()))
            };
            if let Some(old) = old {
                let (child, active) = owner.extension_resolver(&guard);
                let value = tokio::select! {
                    result = (extender.0)(old, child) => result.map_err(|error| Failure::initialization(service, error).error())?,
                    () = owner.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
                };
                drop(active);
                if let Some(event) = owner.publish_extension(&guard, value, None)? {
                    event.dispatch().await?;
                }
                Ok(())
            } else {
                owner.append_extension(&guard, extender).await
            }
        }).await
    }

    fn publish_extension(
        &self,
        guard: &OperationGuard,
        value: Value,
        pending_cursor: Option<usize>,
    ) -> Result<Option<Rebound>> {
        let (previous, event) = {
            let mut state = lock(&guard.inner.state);
            guard.check(&state)?;
            let id = state.id()?;
            let implicit;
            let binding = if let Some(binding) = state.bindings.get(&guard.key.service) {
                Some(binding)
            } else {
                implicit = state.implicit_class_binding(&guard.key.service);
                implicit.as_ref()
            };
            let event = binding.and_then(|binding| {
                Rebound::capture(
                    &state,
                    &guard.inner,
                    self,
                    &guard.key.service,
                    binding,
                    Some(Arc::clone(&value)),
                )
            });
            let attempt = Arc::new(Attempt {
                id,
                service: guard.key.service.clone(),
                outcome: Mutex::new(Some(Ok(value))),
                changed: Notify::new(),
            });
            state.extensions.resolved.insert(guard.key.clone());
            if let Some(cursor) = pending_cursor {
                state
                    .extensions
                    .pending_cursor
                    .insert(guard.key.service.clone(), cursor);
            }
            let previous = state.slots.insert(guard.key.clone(), attempt);
            guard.operation.published.store(true, Ordering::Release);
            (previous, event)
        };
        guard.operation.attempt.changed.notify_waiters();
        drop(previous);
        Ok(event)
    }

    async fn append_extension(&self, guard: &OperationGuard, extender: Extender) -> Result<()> {
        let event = {
            let mut state = lock(&guard.inner.state);
            guard.check(&state)?;
            if guard.local {
                state
                    .extensions
                    .local
                    .entry(guard.key.clone())
                    .or_default()
                    .push(extender);
            } else {
                state
                    .extensions
                    .global
                    .entry(guard.key.service.clone())
                    .or_default()
                    .push(extender);
            }
            let resolved = if guard.local {
                state.extensions.resolved.contains(&guard.key)
            } else {
                state.resolved_history.contains(&guard.key.service)
            };
            guard.operation.published.store(true, Ordering::Release);
            resolved
                .then(|| {
                    let implicit;
                    let binding = if let Some(binding) = state.bindings.get(&guard.key.service) {
                        Some(binding)
                    } else {
                        implicit = state.implicit_class_binding(&guard.key.service);
                        implicit.as_ref()
                    }?;
                    Rebound::capture(
                        &state,
                        &guard.inner,
                        self,
                        &guard.key.service,
                        binding,
                        None,
                    )
                })
                .flatten()
        };
        guard.operation.attempt.changed.notify_waiters();
        if let Some(event) = event {
            event.dispatch().await?;
        }
        Ok(())
    }

    pub(super) async fn forget_chain(&self, service: &ServiceKey, local: bool) -> Result<()> {
        let inner = self.container()?.inner;
        let service = lock(&inner.state).alias_key(service)?;
        let service = &service;
        let guard = self.operation(&inner, service, local, None).await?;
        guard.owner.wait_constructions(&guard).await?;
        let removed = {
            let mut state = lock(&inner.state);
            guard.check(&state)?;
            if guard.local {
                state.extensions.local.remove(&guard.key)
            } else {
                state.extensions.global.remove(service)
            }
        };
        drop(removed);
        drop(guard);
        Ok(())
    }
}

impl Resolver {
    pub(super) async fn initialize_instance(
        &self,
        inner: &Arc<Inner>,
        source: &BindingSource,
    ) -> Result<bool> {
        self.initialize_instance_until(inner, source, source.binding.pending.len())
            .await
    }

    async fn initialize_instance_until(
        &self,
        inner: &Arc<Inner>,
        source: &BindingSource,
        through: usize,
    ) -> Result<bool> {
        let service = &source.key;
        let key = CacheKey {
            service: service.clone(),
            scope: u64::MAX,
        };
        {
            let state = lock(&inner.state);
            self.check_open(&state)?;
            if !source.is_current(&state) {
                return Err(container_error(
                    "binding_replaced",
                    "初始化期间服务绑定已更换",
                ));
            }
            if state.extensions.instance_complete(service, through) {
                return Ok(false);
            }
            // 同一操作的通知读取已经发布的中间值，不再次驱动剩余队列。
            if recursive_extension(inner, &key)
                && state
                    .extensions
                    .operations
                    .get(&key)
                    .is_some_and(|operation| operation.published.load(Ordering::Acquire))
            {
                return Ok(false);
            }
        }
        let guard = self.operation(inner, service, false, Some(source)).await?;
        let mut stack = EXTENDING.try_with(Clone::clone).unwrap_or_default();
        stack.push((address(inner), key.clone(), guard.operation.attempt.id));
        EXTENDING
            .scope(stack, self.run_pending(&guard, &source.binding, through))
            .await?;
        Ok(true)
    }

    async fn run_pending(
        &self,
        guard: &OperationGuard,
        binding: &Binding,
        through: usize,
    ) -> Result<()> {
        loop {
            let (index, old) = {
                let state = lock(&guard.inner.state);
                guard.check(&state)?;
                let index = state
                    .extensions
                    .pending_cursor
                    .get(&guard.key.service)
                    .copied()
                    .unwrap_or(0);
                if index >= through {
                    return Ok(());
                }
                let value = state
                    .slots
                    .get(&guard.key)
                    .and_then(|slot| {
                        lock(&slot.outcome)
                            .as_ref()
                            .and_then(|value| value.as_ref().ok().cloned())
                    })
                    .or_else(|| binding.instance.clone())
                    .ok_or_else(|| {
                        container_error("service_type_mismatch", "待装饰 instance 不存在")
                    })?;
                guard.operation.published.store(false, Ordering::Release);
                (index, value)
            };
            let (child, active) = self.extension_resolver(guard);
            let value = tokio::select! {
                result = (binding.pending[index].0)(old, child) => result.map_err(|error| Failure::initialization(&guard.key.service, error).error())?,
                () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
            };
            drop(active);
            if let Some(event) = self.publish_extension(guard, value, Some(index + 1))? {
                event.dispatch().await?;
            }
        }
    }
}

#[cfg(test)]
mod service_key_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn named_container(name: String) -> Result<(Container, ServiceKey)> {
        let key = ServiceKey::named(name);
        let mut builder = ContainerBuilder::new();
        builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(7)) }));
        let binding = builder
            .bindings
            .remove(&ServiceKey::typed::<usize>())
            .ok_or_else(|| container_error("test_binding_missing", "缺少测试绑定"))?;
        builder.bindings.insert(key.clone(), binding);
        Ok((builder.build(), key))
    }

    fn integer(value: &Value) -> Result<usize> {
        value
            .downcast_ref::<Arc<usize>>()
            .map(|value| **value)
            .ok_or_else(|| container_error("test_value_type", "测试值类型不符"))
    }

    #[tokio::test]
    async fn owned_named_identity_reuses_cache_extension_and_rebinding_generation() -> Result<()> {
        let (container, key) = named_container(String::from("mail.transport"))?;
        let resolver = container.root_resolver()?;
        let first = resolver.resolve_value(key.clone()).await?;
        let second = resolver.resolve_value(key.clone()).await?;
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!container.contains::<usize>());
        container.instance(Arc::new(123_usize)).await?;
        assert_eq!(*container.resolve::<usize>().await?, 123);
        assert!(Arc::ptr_eq(
            &first,
            &resolver.resolve_value(key.clone()).await?
        ));
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        lock(&container.inner.state)
            .extensions
            .listeners
            .entry(key.clone())
            .or_default()
            .push(erase_rebinding::<usize, _>(move |_, _| {
                let calls = Arc::clone(&observed);
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            }));
        resolver
            .extend_value(
                &key,
                erase_extender::<usize, _>(|value, _| {
                    Box::pin(async move { Ok(Arc::new(*value + 1)) })
                }),
                false,
            )
            .await?;
        assert_eq!(integer(&resolver.resolve_value(key.clone()).await?)?, 8);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mut replacement = lock(&container.inner.state)
            .bindings
            .get(&key)
            .cloned()
            .ok_or_else(|| container_error("test_binding_missing", "缺少测试绑定"))?;
        replacement.factory = Some(Arc::new(|_| {
            Box::pin(async { Ok(super::super::erase(Arc::new(99_usize))) })
        }));
        container.replace_binding(key.clone(), replacement).await?;
        assert_eq!(integer(&resolver.resolve_value(key.clone()).await?)?, 99);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(integer(&first)?, 7);
        Ok(())
    }

    #[tokio::test]
    async fn named_pending_instance_uses_the_same_order_and_cursor_directory() -> Result<()> {
        let key = ServiceKey::named(String::from("scoped.instance"));
        let mut builder = ContainerBuilder::new();
        builder.instance(Arc::new(4_usize));
        let mut binding = builder
            .bindings
            .remove(&ServiceKey::typed::<usize>())
            .ok_or_else(|| container_error("test_binding_missing", "缺少测试绑定"))?;
        binding.pending.push(erase_extender::<usize, _>(|value, _| {
            Box::pin(async move { Ok(Arc::new(*value + 3)) })
        }));
        builder.bindings.insert(key.clone(), binding);
        builder.extensions.pending_order.push((key.clone(), 1));
        let container = builder.build();
        container.initialize_extensions().await?;
        let resolver = container.root_resolver()?;
        assert_eq!(integer(&resolver.resolve_value(key.clone()).await?)?, 7);
        assert!(
            lock(&container.inner.state)
                .extensions
                .instance_complete(&key, 1)
        );
        Ok(())
    }

    #[tokio::test]
    async fn dynamic_named_cycle_retains_owned_service_chain() -> Result<()> {
        let original = format!("cycle.named\n{}", "x".repeat(256));
        let (container, key) = named_container(original.clone())?;
        assert_eq!(key.name(), original);
        let nested = key.clone();
        let mut replacement = lock(&container.inner.state)
            .bindings
            .get(&key)
            .cloned()
            .ok_or_else(|| container_error("test_binding_missing", "缺少测试绑定"))?;
        replacement.factory = Some(Arc::new(move |resolver| {
            let nested = nested.clone();
            Box::pin(async move { resolver.resolve_value(nested).await })
        }));
        container.replace_binding(key.clone(), replacement).await?;
        let error = container
            .root_resolver()?
            .resolve_value(key)
            .await
            .err()
            .ok_or_else(|| container_error("test_cycle_missing", "命名环应失败"))?;
        assert_eq!(error.code(), "service_cycle");
        let visible = error.to_string();
        assert!(visible.contains("cycle.named"));
        assert!(visible.contains("\\n"));
        assert!(!visible.contains('\n'));
        assert!(!visible.contains(&"x".repeat(129)));
        Ok(())
    }

    #[tokio::test]
    async fn named_factory_failure_keeps_exact_structured_identity_and_source() -> Result<()> {
        let name = format!("failed.named\n{}", "x".repeat(256));
        let (container, key) = named_container(name.clone())?;
        let mut replacement = lock(&container.inner.state)
            .bindings
            .get(&key)
            .cloned()
            .ok_or_else(|| container_error("test_binding_missing", "缺少测试绑定"))?;
        replacement.factory = Some(Arc::new(|_| {
            Box::pin(async {
                Err(Error::custom(
                    "private_factory_fixture",
                    "private-source-sentinel",
                ))
            })
        }));
        container.replace_binding(key.clone(), replacement).await?;
        let error = container
            .root_resolver()?
            .resolve_value(key)
            .await
            .err()
            .ok_or_else(|| container_error("test_factory_error_missing", "命名工厂应失败"))?;
        let Error::ServiceInitialization { service, source } = &error else {
            return Err(container_error(
                "test_structured_error_missing",
                "应保留结构化错误",
            ));
        };
        assert_eq!(service.name(), name);
        assert_eq!(source.code(), "private_factory_fixture");
        let visible = format!("{error} {error:?}");
        assert!(!visible.contains("private-source-sentinel"));
        assert!(!visible.contains('\n'));
        assert!(!visible.contains(&"x".repeat(129)));
        Ok(())
    }
}

#[cfg(test)]
#[path = "callback_disposal_tests.rs"]
mod callback_disposal_tests;

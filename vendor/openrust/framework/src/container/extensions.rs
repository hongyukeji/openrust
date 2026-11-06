//! Binding 装饰沿用容器的缓存键、等待图和生命周期，不持内部锁执行用户代码。

use super::{
    ActiveFactory, Arc, AtomicBool, Attempt, Binding, BoxFuture, CacheKey, Container,
    ContainerBuilder, Error, Failure, HashMap, HashSet, Inner, Lifetime, Mutex, Notify, Ordering,
    Resolver, Result, Scope, ScopeStatus, State, TypeId, Value, WaitEdge, container_error, lock,
    type_name,
};

type ExtensionFn = dyn Fn(Value, Resolver) -> BoxFuture<'static, Result<Value>> + Send + Sync;
#[derive(Clone)]
pub(super) struct Extender(Arc<ExtensionFn>);
type RebindingFn = dyn Fn(Resolver, Value) -> BoxFuture<'static, Result<()>> + Send + Sync;
#[derive(Clone)]
pub(super) struct Rebinding(Arc<RebindingFn>);

tokio::task_local! {
    // 回调在发布后允许同型 resolve，但仍不得递归 extend 自己。
    static EXTENDING: Vec<(usize, CacheKey, u64)>;
}

#[derive(Default)]
pub(super) struct ExtensionState {
    global: HashMap<TypeId, Vec<Extender>>,
    local: HashMap<CacheKey, Vec<Extender>>,
    listeners: HashMap<TypeId, Vec<Rebinding>>,
    pub(super) resolved: HashSet<CacheKey>,
    pending_cursor: HashMap<TypeId, usize>,
    pending_order: Vec<(TypeId, usize)>,
    operations: HashMap<CacheKey, Arc<Operation>>,
    transients: HashMap<CacheKey, Vec<Arc<Attempt>>>,
}

impl ExtensionState {
    pub(super) fn has_rebinding_listeners(&self, service: TypeId) -> bool {
        self.listeners
            .get(&service)
            .is_some_and(|listeners| !listeners.is_empty())
    }

    pub(super) fn discard_operations(&mut self, service: TypeId) -> Vec<Arc<Operation>> {
        let keys: Vec<_> = self
            .operations
            .keys()
            .filter(|key| key.service == service)
            .copied()
            .collect();
        keys.into_iter()
            .filter_map(|key| self.operations.remove(&key))
            .collect()
    }
    pub(super) fn discard_pending(&mut self, service: TypeId) {
        self.pending_order
            .retain(|(registered, _)| *registered != service);
        self.pending_cursor.remove(&service);
    }

    pub(super) fn instance_complete(&self, service: TypeId, total: usize) -> bool {
        self.pending_cursor.get(&service).copied().unwrap_or(0) >= total
    }

    pub(super) fn pending_operation(&self, key: CacheKey) -> bool {
        self.operations
            .get(&key)
            .is_some_and(|operation| !operation.published.load(Ordering::Acquire))
    }

    pub(super) fn discard_scope(&mut self, scope: u64) -> Vec<Extender> {
        self.resolved.retain(|key| key.scope != scope);
        let keys: Vec<_> = self
            .local
            .keys()
            .filter(|key| key.scope == scope)
            .copied()
            .collect();
        keys.into_iter()
            .filter_map(|key| self.local.remove(&key))
            .flatten()
            .collect()
    }

    fn chain(&self, key: CacheKey) -> Vec<Extender> {
        self.global
            .get(&key.service)
            .into_iter()
            .flatten()
            .chain(self.local.get(&key).into_iter().flatten())
            .cloned()
            .collect()
    }
}

fn erase_extender<T, F>(extender: F) -> Extender
where
    T: ?Sized + Send + Sync + 'static,
    F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
{
    let extender = Arc::new(extender);
    Extender(Arc::new(move |value, resolver| {
        let value = value.downcast_ref::<Arc<T>>().cloned();
        let extender = Arc::clone(&extender);
        Box::pin(async move {
            let value = value
                .ok_or_else(|| container_error("service_type_mismatch", "装饰器输入类型不匹配"))?;
            Ok(Arc::new(extender(value, resolver).await?) as Value)
        })
    }))
}

fn erase_rebinding<T, F>(callback: F) -> Rebinding
where
    T: ?Sized + Send + Sync + 'static,
    F: Fn(Resolver, Arc<T>) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
{
    let callback = Arc::new(callback);
    Rebinding(Arc::new(move |resolver, value| {
        let value = value.downcast_ref::<Arc<T>>().cloned();
        let callback = Arc::clone(&callback);
        Box::pin(async move {
            let value = value
                .ok_or_else(|| container_error("service_type_mismatch", "重绑定通知类型不匹配"))?;
            callback(resolver, value).await
        })
    }))
}

impl ContainerBuilder {
    /// 装饰原 factory；显式 instance 的异步变换延至 Provider boot 前或首次解析。
    ///
    /// 未绑定类型也能登记；factory 覆盖保留持久链，instance 覆盖丢弃旧即时链。
    pub fn extend<T, F>(&mut self, extender: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        let extender = erase_extender::<T, F>(extender);
        if let Some(binding) = self
            .bindings
            .get_mut(&TypeId::of::<T>())
            .filter(|binding| binding.instance.is_some())
        {
            binding.pending.push(extender);
            self.extensions
                .pending_order
                .push((TypeId::of::<T>(), binding.pending.len()));
        } else {
            self.extensions
                .global
                .entry(TypeId::of::<T>())
                .or_default()
                .push(extender);
        }
        self
    }

    /// 清除持久装饰链，不撤销当前 instance 或其待执行即时装饰。
    pub fn forget_extenders<T: ?Sized + Send + Sync + 'static>(&mut self) -> &mut Self {
        self.extensions.global.remove(&TypeId::of::<T>());
        self
    }

    /// 登记装饰成功后的异步通知；新缓存发布后按登记顺序执行。
    pub fn rebinding<T, F>(&mut self, callback: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver, Arc<T>) -> BoxFuture<'static, Result<()>> + Send + Sync + 'static,
    {
        self.extensions
            .listeners
            .entry(TypeId::of::<T>())
            .or_default()
            .push(erase_rebinding::<T, F>(callback));
        self
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
            *outcome = Some(Ok(Arc::new(())));
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

fn recursive_extension(inner: &Arc<Inner>, key: CacheKey) -> bool {
    EXTENDING
        .try_with(|stack| {
            stack
                .iter()
                .any(|(owner, active, _)| *owner == address(inner) && *active == key)
        })
        .unwrap_or(false)
}

impl Resolver {
    fn check_open(&self, state: &State) -> Result<()> {
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        if self.closed.closed.load(Ordering::Acquire) {
            return Err(container_error("scope_closed", "生命周期已关闭"));
        }
        Ok(())
    }

    fn wait_edge(
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
                    state
                        .slots
                        .values()
                        .chain(
                            state
                                .extensions
                                .operations
                                .values()
                                .map(|operation| &operation.attempt),
                        )
                        .chain(state.extensions.transients.values().flatten())
                        .find(|slot| slot.id == id)
                        .map(|slot| slot.name)
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
        key: CacheKey,
        name: &'static str,
        generation: u64,
    ) -> Result<Option<(Arc<Attempt>, bool, Option<WaitEdge>)>> {
        loop {
            let waiting = {
                let mut state = lock(&inner.state);
                self.check_open(&state)?;
                if state
                    .bindings
                    .get(&key.service)
                    .map(|binding| binding.generation)
                    != Some(generation)
                {
                    return Ok(None);
                }
                if let Some(operation) = state
                    .extensions
                    .operations
                    .get(&key)
                    .filter(|op| !op.published.load(Ordering::Acquire))
                    .cloned()
                {
                    let edge = self.wait_edge(inner, &mut state, &operation.attempt)?;
                    Some((operation, edge))
                } else {
                    let (attempt, owner) = if let Some(attempt) = state.slots.get(&key) {
                        (Arc::clone(attempt), false)
                    } else {
                        let attempt = Arc::new(Attempt {
                            id: state.id()?,
                            name,
                            outcome: Mutex::new(None),
                            changed: Notify::new(),
                        });
                        state.slots.insert(key, Arc::clone(&attempt));
                        (attempt, true)
                    };
                    let edge = self.wait_edge(inner, &mut state, &attempt)?;
                    return Ok(Some((attempt, owner, edge)));
                }
            };
            if let Some((operation, _edge)) = waiting {
                self.wait_visible(&operation).await?;
            }
        }
    }

    pub(super) async fn construct_value(&self, binding: &Binding, key: CacheKey) -> Result<Value> {
        let container = self.container()?;
        // 显式 instance 跳过持久链；它只运行其后登记的即时链。
        let chain = if binding.instance.is_some() {
            Vec::new()
        } else {
            lock(&container.inner.state).extensions.chain(key)
        };
        let mut value = (binding.factory)(self.clone()).await?;
        for extender in chain {
            value = (extender.0)(value, self.clone()).await?;
        }
        Ok(value)
    }

    pub(super) async fn transient(
        &self,
        binding: &Binding,
        mut child: Self,
        service: TypeId,
        name: &'static str,
        activity: Arc<AtomicBool>,
    ) -> Result<Option<Value>> {
        let inner = self.container()?.inner;
        let key = CacheKey {
            service,
            scope: self.scope,
        };
        let (attempt, _edge) = loop {
            let waiting = {
                let mut state = lock(&inner.state);
                self.check_open(&state)?;
                if state
                    .bindings
                    .get(&service)
                    .map(|current| current.generation)
                    != Some(binding.generation)
                {
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
                        name,
                        outcome: Mutex::new(None),
                        changed: Notify::new(),
                    });
                    state
                        .extensions
                        .transients
                        .entry(key)
                        .or_default()
                        .push(Arc::clone(&attempt));
                    let edge = self.wait_edge(&inner, &mut state, &attempt)?;
                    break (attempt, edge);
                }
            };
            if let Some((operation, _edge)) = waiting {
                self.wait_visible(&operation).await?;
            }
        };
        let mut guard = TransientGuard {
            inner: Arc::clone(&inner),
            key,
            attempt: Arc::clone(&attempt),
            generation: binding.generation,
            activity,
            closed: Arc::clone(&self.closed),
            finished: false,
        };
        child.constructing = Some((attempt.id, Arc::clone(&guard.activity)));
        let result = tokio::select! {
            result = child.construct_value(binding, key) => result,
            () = self.closed.wait_closed() => Err(container_error("scope_closed", "生命周期已关闭")),
        };
        guard.finish(result.as_ref().map(Arc::clone).map_err(|error| {
            Failure::initialization(name, Error::custom(error.code(), error.to_string()))
        }));
        result.map(Some)
    }
}

struct TransientGuard {
    inner: Arc<Inner>,
    key: CacheKey,
    attempt: Arc<Attempt>,
    generation: u64,
    activity: Arc<AtomicBool>,
    closed: Arc<ScopeStatus>,
    finished: bool,
}

impl TransientGuard {
    fn finish(&mut self, result: std::result::Result<Value, Failure>) {
        self.activity.store(false, Ordering::Release);
        let removed = {
            let mut state = lock(&self.inner.state);
            if result.is_ok()
                && !state.terminated
                && !self.closed.closed.load(Ordering::Acquire)
                && state
                    .bindings
                    .get(&self.key.service)
                    .map(|binding| binding.generation)
                    == Some(self.generation)
            {
                state.extensions.resolved.insert(self.key);
            }
            state.edges.retain(|(from, _), _| *from != self.attempt.id);
            // 保留 guard 的 Arc，确保最后一个用户 Value 不会在内部锁内释放。
            let removed = state
                .extensions
                .transients
                .get_mut(&self.key)
                .and_then(|items| {
                    items
                        .iter()
                        .position(|item| item.id == self.attempt.id)
                        .map(|index| items.remove(index))
                });
            if state
                .extensions
                .transients
                .get(&self.key)
                .is_some_and(Vec::is_empty)
            {
                state.extensions.transients.remove(&self.key);
            }
            removed
        };
        *lock(&self.attempt.outcome) = Some(result);
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
        let bound = {
            let mut state = lock(&self.inner.state);
            if state.terminated {
                return Err(container_error("container_closed", "容器已关闭"));
            }
            state
                .extensions
                .listeners
                .entry(TypeId::of::<T>())
                .or_default()
                .push(erase_rebinding::<T, F>(callback));
            state.bindings.contains_key(&TypeId::of::<T>())
        };
        if bound {
            self.resolve::<T>().await.map(Some)
        } else {
            Ok(None)
        }
    }

    fn root_resolver(&self) -> Result<Resolver> {
        let state = lock(&self.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        Ok(self.resolver(
            state.root_scope,
            Arc::clone(&state.root_closed),
            state.root_resources.registrar(),
        ))
    }

    /// 装饰当前共享缓存；未缓存时保留持久链供未来构造，现有兄弟 scope 不广播替换。
    ///
    /// 失败或取消不替换旧缓存；已发布后的 rebinding 错误不会回滚新值。
    pub async fn extend<T, F>(&self, extender: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.root_resolver()?
            .extend_value(
                TypeId::of::<T>(),
                type_name::<T>(),
                erase_extender::<T, F>(extender),
                false,
            )
            .await
    }

    /// 清除未来构造的持久链；不撤销当前缓存或 instance 的即时变换。
    pub async fn forget_extenders<T: ?Sized + Send + Sync + 'static>(&self) -> Result<()> {
        self.root_resolver()?
            .forget_chain(TypeId::of::<T>(), type_name::<T>(), false)
            .await
    }

    pub(crate) async fn initialize_extensions(&self) -> Result<()> {
        // 只保存实例即时操作的登记顺序，不让 HashMap 随机遍历改变跨类型副作用。
        let pending = lock(&self.inner.state).extensions.pending_order.clone();
        for (service, through) in pending {
            let binding = lock(&self.inner.state).bindings.get(&service).cloned();
            if let Some(binding) = binding {
                self.root_resolver()?
                    .initialize_instance_until(&self.inner, service, &binding, through)
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
        if self.closed.closed.load(Ordering::Acquire) {
            return Err(container_error("scope_closed", "生命周期已关闭"));
        }
        let singleton = lock(&self.container.inner.state)
            .bindings
            .get(&TypeId::of::<T>())
            .is_some_and(|binding| binding.lifetime == Lifetime::Singleton);
        if singleton {
            return self.container.extend::<T, F>(extender).await;
        }
        let resolver = self.container.resolver(
            self.id,
            Arc::clone(&self.closed),
            self.resources.registrar(),
        );
        resolver
            .extend_value(
                TypeId::of::<T>(),
                type_name::<T>(),
                erase_extender::<T, F>(extender),
                true,
            )
            .await
    }

    /// 仅清除本 scope 的持久装饰；singleton 的持久链由应用清除。
    pub async fn forget_extenders<T: ?Sized + Send + Sync + 'static>(&self) -> Result<()> {
        if self.closed.closed.load(Ordering::Acquire) {
            return Err(container_error("scope_closed", "生命周期已关闭"));
        }
        let singleton = lock(&self.container.inner.state)
            .bindings
            .get(&TypeId::of::<T>())
            .is_some_and(|binding| binding.lifetime == Lifetime::Singleton);
        if singleton {
            return self.container.forget_extenders::<T>().await;
        }
        self.container
            .resolver(
                self.id,
                Arc::clone(&self.closed),
                self.resources.registrar(),
            )
            .forget_chain(TypeId::of::<T>(), type_name::<T>(), true)
            .await
    }
}

impl Resolver {
    fn extension_key(&self, inner: &Inner, service: TypeId) -> CacheKey {
        let singleton = lock(&inner.state)
            .bindings
            .get(&service)
            .is_some_and(|binding| binding.lifetime == Lifetime::Singleton);
        CacheKey {
            service,
            scope: if singleton { u64::MAX } else { self.scope },
        }
    }

    async fn operation(
        &self,
        inner: &Arc<Inner>,
        key: CacheKey,
        name: &'static str,
    ) -> Result<OperationGuard> {
        if recursive_extension(inner, key) {
            return Err(container_error(
                "service_cycle",
                format!("服务 `{name}` 不能递归装饰自己"),
            ));
        }
        loop {
            let waiting = {
                let mut state = lock(&inner.state);
                self.check_open(&state)?;
                if let Some(operation) = state.extensions.operations.get(&key).cloned() {
                    let edge = self.wait_edge(inner, &mut state, &operation.attempt)?;
                    Some((operation, edge))
                } else {
                    let operation = Arc::new(Operation {
                        attempt: Arc::new(Attempt {
                            id: state.id()?,
                            name,
                            outcome: Mutex::new(None),
                            changed: Notify::new(),
                        }),
                        published: AtomicBool::new(false),
                    });
                    state
                        .extensions
                        .operations
                        .insert(key, Arc::clone(&operation));
                    return Ok(OperationGuard {
                        inner: Arc::clone(inner),
                        key,
                        operation,
                        generation: state
                            .bindings
                            .get(&key.service)
                            .map_or(0, |binding| binding.generation),
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
            self.check_open(&state)?;
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
                    .cloned(),
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
                () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
            }
        }
        Ok(())
    }

    fn extension_resolver(
        &self,
        inner: &Arc<Inner>,
        service: TypeId,
        name: &'static str,
        id: u64,
    ) -> (Self, ActiveFactory) {
        let mut child = self.clone();
        child
            .chain
            .retain(|(_, _, active)| active.load(Ordering::Acquire));
        let active = Arc::new(AtomicBool::new(true));
        child.chain.push((service, name, Arc::clone(&active)));
        child.constructing = Some((id, Arc::clone(&active)));
        if lock(&inner.state)
            .bindings
            .get(&service)
            .is_some_and(|binding| binding.lifetime == Lifetime::Singleton)
        {
            child.captive = true;
            child.scope = u64::MAX;
            child.resources.clone_from(&inner.resources);
        }
        (child, ActiveFactory(active))
    }

    async fn extend_value(
        &self,
        service: TypeId,
        name: &'static str,
        extender: Extender,
        local: bool,
    ) -> Result<()> {
        let inner = self.container()?.inner;
        let pending = lock(&inner.state).bindings.get(&service).cloned();
        if let Some(binding) = pending.filter(|binding| !binding.pending.is_empty()) {
            self.initialize_instance(&inner, service, &binding).await?;
        }
        let key = self.extension_key(&inner, service);
        let guard = self.operation(&inner, key, name).await?;
        let mut stack = EXTENDING.try_with(Clone::clone).unwrap_or_default();
        stack.push((address(&inner), key, guard.operation.attempt.id));
        EXTENDING.scope(stack, async {
            self.wait_constructions(&guard).await?;
            let old = {
                let state = lock(&inner.state);
                self.check_open(&state)?;
                state.slots.get(&key).and_then(|slot| lock(&slot.outcome).as_ref().and_then(|value| value.as_ref().ok().cloned()))
                    .or_else(|| state.bindings.get(&service).filter(|binding| binding.pending.is_empty()).and_then(|binding| binding.instance.clone()))
            };
            if let Some(old) = old {
                let (child, active) = self.extension_resolver(&inner, service, name, guard.operation.attempt.id);
                let value = tokio::select! {
                    result = (extender.0)(old, child) => result.map_err(|error| Failure::initialization(name, error).error())?,
                    () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
                };
                drop(active);
                self.publish_extension(&guard, Arc::clone(&value), name, None)?;
                self.notify_rebinding(&inner, service, value).await
            } else {
                self.append_extension(&guard, extender, local, name).await
            }
        }).await
    }

    fn publish_extension(
        &self,
        guard: &OperationGuard,
        value: Value,
        name: &'static str,
        pending_cursor: Option<usize>,
    ) -> Result<()> {
        let previous = {
            let mut state = lock(&guard.inner.state);
            self.check_open(&state)?;
            if state
                .bindings
                .get(&guard.key.service)
                .map_or(0, |binding| binding.generation)
                != guard.generation
            {
                return Err(container_error(
                    "binding_replaced",
                    "装饰期间服务绑定已更换",
                ));
            }
            let attempt = Arc::new(Attempt {
                id: state.id()?,
                name,
                outcome: Mutex::new(Some(Ok(value))),
                changed: Notify::new(),
            });
            state.extensions.resolved.insert(guard.key);
            if let Some(cursor) = pending_cursor {
                state
                    .extensions
                    .pending_cursor
                    .insert(guard.key.service, cursor);
            }
            let previous = state.slots.insert(guard.key, attempt);
            guard.operation.published.store(true, Ordering::Release);
            previous
        };
        guard.operation.attempt.changed.notify_waiters();
        drop(previous);
        Ok(())
    }

    async fn append_extension(
        &self,
        guard: &OperationGuard,
        extender: Extender,
        local: bool,
        name: &'static str,
    ) -> Result<()> {
        let remake = {
            let mut state = lock(&guard.inner.state);
            self.check_open(&state)?;
            if state
                .bindings
                .get(&guard.key.service)
                .map_or(0, |binding| binding.generation)
                != guard.generation
            {
                return Err(container_error(
                    "binding_replaced",
                    "装饰期间服务绑定已更换",
                ));
            }
            if local && guard.key.scope != u64::MAX {
                state
                    .extensions
                    .local
                    .entry(guard.key)
                    .or_default()
                    .push(extender);
            } else {
                state
                    .extensions
                    .global
                    .entry(guard.key.service)
                    .or_default()
                    .push(extender);
            }
            let resolved = if local && guard.key.scope != u64::MAX {
                state.extensions.resolved.contains(&guard.key)
            } else {
                state
                    .extensions
                    .resolved
                    .iter()
                    .any(|key| key.service == guard.key.service)
            };
            let remake = resolved
                && state
                    .extensions
                    .listeners
                    .get(&guard.key.service)
                    .is_some_and(|listeners| !listeners.is_empty());
            guard.operation.published.store(true, Ordering::Release);
            remake
        };
        guard.operation.attempt.changed.notify_waiters();
        if remake {
            let value = self.resolve_value(guard.key.service, name).await?;
            self.notify_rebinding(&guard.inner, guard.key.service, value)
                .await?;
        }
        Ok(())
    }

    pub(super) async fn notify_rebinding(
        &self,
        inner: &Arc<Inner>,
        service: TypeId,
        value: Value,
    ) -> Result<()> {
        let callbacks = lock(&inner.state)
            .extensions
            .listeners
            .get(&service)
            .cloned()
            .unwrap_or_default();
        let mut resolver = self.clone();
        resolver.chain.clear();
        resolver.constructing = None;
        if lock(&inner.state)
            .bindings
            .get(&service)
            .is_some_and(|binding| binding.lifetime == Lifetime::Singleton)
        {
            resolver.captive = true;
            resolver.scope = u64::MAX;
            resolver.resources = inner.resources.clone();
        }
        for callback in callbacks {
            tokio::select! {
                result = (callback.0)(resolver.clone(), Arc::clone(&value)) => result?,
                () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
            }
        }
        Ok(())
    }

    async fn forget_chain(&self, service: TypeId, name: &'static str, local: bool) -> Result<()> {
        let inner = self.container()?.inner;
        let key = self.extension_key(&inner, service);
        let guard = self.operation(&inner, key, name).await?;
        self.wait_constructions(&guard).await?;
        let removed = {
            let mut state = lock(&inner.state);
            self.check_open(&state)?;
            if local && key.scope != u64::MAX {
                state.extensions.local.remove(&key)
            } else {
                state.extensions.global.remove(&service)
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
        service: TypeId,
        binding: &Binding,
    ) -> Result<()> {
        self.initialize_instance_until(inner, service, binding, binding.pending.len())
            .await
    }

    async fn initialize_instance_until(
        &self,
        inner: &Arc<Inner>,
        service: TypeId,
        binding: &Binding,
        through: usize,
    ) -> Result<()> {
        let key = CacheKey {
            service,
            scope: u64::MAX,
        };
        {
            let state = lock(&inner.state);
            self.check_open(&state)?;
            if state
                .bindings
                .get(&service)
                .map(|current| current.generation)
                != Some(binding.generation)
            {
                return Err(container_error(
                    "binding_replaced",
                    "初始化期间服务绑定已更换",
                ));
            }
            if state.extensions.instance_complete(service, through) {
                return Ok(());
            }
            // 同一操作的通知读取已经发布的中间值，不再次驱动剩余队列。
            if recursive_extension(inner, key)
                && state
                    .extensions
                    .operations
                    .get(&key)
                    .is_some_and(|operation| operation.published.load(Ordering::Acquire))
            {
                return Ok(());
            }
        }
        let guard = self.operation(inner, key, binding.name).await?;
        if guard.generation != binding.generation {
            return Err(container_error(
                "binding_replaced",
                "初始化期间服务绑定已更换",
            ));
        }
        let mut stack = EXTENDING.try_with(Clone::clone).unwrap_or_default();
        stack.push((address(inner), key, guard.operation.attempt.id));
        EXTENDING
            .scope(stack, self.run_pending(&guard, binding, through))
            .await
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
                self.check_open(&state)?;
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
            let (child, active) = self.extension_resolver(
                &guard.inner,
                guard.key.service,
                binding.name,
                guard.operation.attempt.id,
            );
            let value = tokio::select! {
                result = (binding.pending[index].0)(old, child) => result.map_err(|error| Failure::initialization(binding.name, error).error())?,
                () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
            };
            drop(active);
            self.publish_extension(guard, Arc::clone(&value), binding.name, Some(index + 1))?;
            self.notify_rebinding(&guard.inner, guard.key.service, value)
                .await?;
        }
    }
}

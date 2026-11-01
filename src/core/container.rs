//! 类型化服务容器基元；生产应用接入由统一 Provider 迁移完成。
//!
//! ```
//! use openrust::core::container::ContainerBuilder;
//! use std::sync::Arc;
//!
//! # async fn example() -> openrust::Result<()> {
//! let mut bindings = ContainerBuilder::new();
//! bindings.scoped::<String, _>(|_| Box::pin(async { Ok(Arc::new("业务服务".into())) }));
//! let container = bindings.build();
//! let scope = container.scope()?;
//! let service = scope.resolve::<String>().await?;
//! assert_eq!(&*service, "业务服务");
//! scope.close();
//! # Ok(())
//! # }
//! ```

use std::any::{Any, TypeId, type_name};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use tokio::sync::Notify;

tokio::task_local! {
    static ENTRY_SCOPE: Arc<Scope>;
}

use super::resources::{ResourceOwner, ResourceRegistrar};
use crate::config::{ConfigError, ServerConfig, repository::ConfigRepository};

use super::{BoxFuture, Error, Result};

type Value = Arc<dyn Any + Send + Sync>;
type Factory = Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

mod extensions;
use extensions::{Extender, ExtensionState};

fn container_error(code: &str, message: impl Into<String>) -> Error {
    Error::custom_with_status(http::StatusCode::INTERNAL_SERVER_ERROR, code, message)
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lifetime {
    Bind,
    Singleton,
    Scoped,
}

#[derive(Clone)]
struct Binding {
    generation: u64,
    name: &'static str,
    pending: Vec<Extender>,
    lifetime: Lifetime,
    factory: Factory,
    instance: Option<Value>,
}

/// 注册启动阶段服务定义；消费 builder 后得到可动态重绑的同一容器。
#[derive(Default)]
pub struct ContainerBuilder {
    bindings: HashMap<TypeId, Binding>,
    extensions: ExtensionState,
    config: Option<ConfigRepository>,
    resources: Option<ResourceRegistrar>,
}

impl ContainerBuilder {
    /// 创建空定义集合。
    pub fn new() -> Self {
        Self::default()
    }

    /// 给所有工厂共享启动配置视图和应用级资源登记入口。
    pub fn context(&mut self, config: ConfigRepository, resources: ResourceRegistrar) -> &mut Self {
        self.config = Some(config);
        self.resources = Some(resources);
        self
    }

    /// 每次解析调用一次工厂，同类型后注册覆盖前定义。
    pub fn bind<T, F>(&mut self, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register::<T, F>(Lifetime::Bind, factory)
    }

    /// 同一容器共享一个成功实例；不同容器互不共享。
    pub fn singleton<T, F>(&mut self, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register::<T, F>(Lifetime::Singleton, factory)
    }

    /// 同一生命周期共享一个实例，根容器也拥有自己的生命周期。
    pub fn scoped<T, F>(&mut self, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register::<T, F>(Lifetime::Scoped, factory)
    }

    /// 注册调用者已创建的共享实例，允许绑定 `dyn Trait`。
    pub fn instance<T: ?Sized + Send + Sync + 'static>(&mut self, value: Arc<T>) -> &mut Self {
        let stored: Value = Arc::new(Arc::clone(&value));
        self.extensions.discard_pending(TypeId::of::<T>());
        self.bindings.insert(
            TypeId::of::<T>(),
            Binding {
                generation: 0,
                name: type_name::<T>(),
                pending: Vec::new(),
                lifetime: Lifetime::Singleton,
                factory: Arc::new(move |_| {
                    let value = Arc::clone(&stored);
                    Box::pin(async move { Ok(value) })
                }),
                instance: Some(Arc::new(value)),
            },
        );
        self
    }

    /// 同步读取已登记实例；异步 factory 必须通过 `resolve` 构造。
    pub fn get_instance<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.bindings
            .get(&TypeId::of::<T>())
            .filter(|binding| binding.pending.is_empty())
            .and_then(|binding| binding.instance.as_ref())
            .and_then(|value| value.downcast_ref::<Arc<T>>())
            .cloned()
    }

    /// 检查类型是否已绑定，无论其构造方式。
    pub fn contains<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.bindings.contains_key(&TypeId::of::<T>())
    }

    fn register<T, F>(&mut self, lifetime: Lifetime, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        let factory = Arc::new(factory);
        self.extensions.discard_pending(TypeId::of::<T>());
        self.bindings.insert(
            TypeId::of::<T>(),
            Binding {
                generation: 0,
                name: type_name::<T>(),
                pending: Vec::new(),
                lifetime,
                factory: Arc::new(move |resolver| {
                    let factory = Arc::clone(&factory);
                    Box::pin(async move { Ok(Arc::new(factory(resolver).await?) as Value) })
                }),
                instance: None,
            },
        );
        self
    }

    /// 封闭定义集合，创建根生命周期；没有进程全局容器。
    pub fn build(self) -> Container {
        Container {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    bindings: self.bindings,
                    extensions: self.extensions,
                    ..State::default()
                }),
                reset: tokio::sync::Mutex::new(()),
                config: self.config,
                resources: self.resources,
            }),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    service: TypeId,
    scope: u64,
}

#[derive(Default)]
struct ScopeStatus {
    closed: AtomicBool,
    changed: Notify,
}

impl ScopeStatus {
    fn mark_closed(&self) {
        self.closed.store(true, Ordering::Release);
    }

    async fn wait_closed(&self) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.closed.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

#[derive(Default)]
struct State {
    bindings: HashMap<TypeId, Binding>,
    next_generation: u64,
    next_id: u64,
    terminated: bool,
    root_scope: u64,
    slots: HashMap<CacheKey, Arc<Attempt>>,
    extensions: ExtensionState,
    edges: HashMap<(u64, u64), usize>,
    root_closed: Arc<ScopeStatus>,
    root_resources: Arc<ResourceOwner>,
}

impl State {
    fn id(&mut self) -> Result<u64> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|id| *id != u64::MAX)
            .ok_or_else(|| {
                container_error("container_capacity_exceeded", "容器生命周期标识已耗尽")
            })?;
        Ok(self.next_id)
    }

    fn path(&self, from: u64, to: u64) -> Option<Vec<u64>> {
        let mut pending = vec![vec![from]];
        let mut seen = HashSet::new();
        while let Some(path) = pending.pop() {
            let node = *path.last()?;
            if node == to {
                return Some(path);
            }
            if seen.insert(node) {
                for (_, next) in self.edges.keys().filter(|(a, _)| *a == node) {
                    let mut branch = path.clone();
                    branch.push(*next);
                    pending.push(branch);
                }
            }
        }
        None
    }
}

struct Inner {
    state: Mutex<State>,
    reset: tokio::sync::Mutex<()>,
    config: Option<ConfigRepository>,
    resources: Option<ResourceRegistrar>,
}

#[derive(Clone)]
struct Failure {
    code: &'static str,
    message: String,
    source: Option<(&'static str, Arc<Error>)>,
}

impl Failure {
    fn initialization(name: &'static str, error: Error) -> Self {
        match error.code() {
            "service_not_bound" => Failure {
                code: "service_not_bound",
                message: error.to_string(),
                source: None,
            },
            "service_cycle" => Failure {
                code: "service_cycle",
                message: error.to_string(),
                source: None,
            },
            "captive_scope_dependency" => Failure {
                code: "captive_scope_dependency",
                message: error.to_string(),
                source: None,
            },
            "scope_closed" => Failure {
                code: "scope_closed",
                message: error.to_string(),
                source: None,
            },
            _ => Failure {
                code: "service_initialization_failed",
                message: format!("服务 `{name}` 构造失败（原始值未公开）"),
                source: Some((name, Arc::new(error))),
            },
        }
    }

    fn error(&self) -> Error {
        if let Some((service, source)) = &self.source {
            Error::ServiceInitialization {
                service,
                source: Arc::clone(source),
            }
        } else {
            Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                self.code,
                self.message.clone(),
            )
        }
    }
}

struct Attempt {
    id: u64,
    name: &'static str,
    outcome: Mutex<Option<std::result::Result<Value, Failure>>>,
    changed: Notify,
}

impl Attempt {
    async fn wait(&self) -> Result<Value> {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            // 必须在读取结果前登记 waiter，避免完成通知落在两步之间。
            notified.as_mut().enable();
            if let Some(result) = lock(&self.outcome).as_ref() {
                return result.clone().map_err(|failure| failure.error());
            }
            notified.await;
        }
    }
}

struct Construction {
    inner: Arc<Inner>,
    key: CacheKey,
    attempt: Arc<Attempt>,
    activity: Arc<AtomicBool>,
    finished: bool,
}

impl Construction {
    fn finish(&mut self, result: std::result::Result<Value, Failure>) {
        // 发布结果前结束构造帧；通知可以同步唤醒并重新进入保存的 Resolver。
        self.activity.store(false, Ordering::Release);
        {
            let mut state = lock(&self.inner.state);
            state.edges.retain(|(from, _), _| *from != self.attempt.id);
            if result.is_ok()
                && !state.terminated
                && state
                    .slots
                    .get(&self.key)
                    .is_some_and(|slot| slot.id == self.attempt.id)
            {
                state.extensions.resolved.insert(self.key);
            }
        }
        let failed = result.is_err();
        let mut outcome = lock(&self.attempt.outcome);
        if outcome.is_none() {
            *outcome = Some(result);
        }
        drop(outcome);
        if failed {
            let mut state = lock(&self.inner.state);
            if state
                .slots
                .get(&self.key)
                .is_some_and(|a| a.id == self.attempt.id)
            {
                state.slots.remove(&self.key);
            }
        }
        self.finished = true;
        self.attempt.changed.notify_waiters();
    }
}

impl Drop for Construction {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(Err(Failure {
                code: "service_initialization_cancelled",
                message: "服务构造已取消；后续独立解析可重试".into(),
                source: None,
            }));
        }
    }
}

struct WaitEdge {
    inner: Arc<Inner>,
    edge: (u64, u64),
}

impl Drop for WaitEdge {
    fn drop(&mut self) {
        let mut state = lock(&self.inner.state);
        if let Some(count) = state.edges.get_mut(&self.edge) {
            *count -= 1;
            if *count == 0 {
                state.edges.remove(&self.edge);
            }
        }
    }
}

/// 定义封闭的类型化容器；克隆只共享本容器，不建立全局状态。
#[derive(Clone)]
pub struct Container {
    inner: Arc<Inner>,
}

/// 弱引用同一容器的入口 scope 创建句柄；长寿命 singleton 持有它不会形成引用环。
#[derive(Clone)]
pub struct ScopeFactory {
    inner: Weak<Inner>,
}

impl ScopeFactory {
    /// 为一次独立入口创建新 scope；容器已释放或关闭时返回错误。
    pub fn scope(&self) -> Result<Scope> {
        let inner = self
            .inner
            .upgrade()
            .ok_or_else(|| container_error("container_closed", "容器已关闭"))?;
        Container { inner }.scope()
    }
}

impl Container {
    async fn replace_binding(&self, service: TypeId, mut binding: Binding) -> Result<()> {
        let name = binding.name;
        let (previous, removed, operations, notify) = {
            let mut state = lock(&self.inner.state);
            if state.terminated {
                return Err(container_error("container_closed", "容器已关闭"));
            }
            state.next_generation = state.next_generation.checked_add(1).ok_or_else(|| {
                container_error("container_capacity_exceeded", "服务绑定版本已耗尽")
            })?;
            binding.generation = state.next_generation;
            let notify = state
                .extensions
                .resolved
                .iter()
                .any(|key| key.service == service)
                && state.extensions.has_rebinding_listeners(service);
            state.extensions.discard_pending(service);
            state
                .extensions
                .resolved
                .retain(|key| key.service != service);
            let operations = state.extensions.discard_operations(service);
            state.edges.retain(|(from, to), _| {
                !operations.iter().any(|operation| {
                    let id = operation.attempt_id();
                    *from == id || *to == id
                })
            });
            let keys: Vec<_> = state
                .slots
                .keys()
                .filter(|key| key.service == service)
                .copied()
                .collect();
            let removed: Vec<_> = keys
                .into_iter()
                .filter_map(|key| state.slots.remove(&key))
                .collect();
            let previous = state.bindings.insert(service, binding);
            (previous, removed, operations, notify)
        };
        for operation in &operations {
            operation.release();
        }
        drop(previous);
        drop(removed);
        drop(operations);
        if notify {
            let resolver = {
                let state = lock(&self.inner.state);
                self.resolver(
                    state.root_scope,
                    Arc::clone(&state.root_closed),
                    state.root_resources.registrar(),
                )
            };
            let value = resolver.resolve_value(service, name).await?;
            resolver
                .notify_rebinding(&self.inner, service, value)
                .await?;
        }
        Ok(())
    }

    async fn register<T, F>(&self, lifetime: Lifetime, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        let factory = Arc::new(factory);
        self.replace_binding(
            TypeId::of::<T>(),
            Binding {
                generation: 0,
                name: type_name::<T>(),
                pending: Vec::new(),
                lifetime,
                factory: Arc::new(move |resolver| {
                    let factory = Arc::clone(&factory);
                    Box::pin(async move { Ok(Arc::new(factory(resolver).await?) as Value) })
                }),
                instance: None,
            },
        )
        .await
    }

    /// 动态注册每次解析均调用的工厂；覆盖后新解析使用新定义。
    pub async fn bind<T, F>(&self, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register::<T, F>(Lifetime::Bind, factory).await
    }

    /// 动态注册同一容器共享的服务，清除旧缓存。
    pub async fn singleton<T, F>(&self, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register::<T, F>(Lifetime::Singleton, factory).await
    }

    /// 动态注册各 scope 独立共享的服务，清除所有旧 scope 缓存。
    pub async fn scoped<T, F>(&self, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register::<T, F>(Lifetime::Scoped, factory).await
    }

    /// 动态登记现成实例；旧引用继续有效，新解析取得此实例。
    pub async fn instance<T: ?Sized + Send + Sync + 'static>(&self, value: Arc<T>) -> Result<()> {
        let stored: Value = Arc::new(Arc::clone(&value));
        self.replace_binding(
            TypeId::of::<T>(),
            Binding {
                generation: 0,
                name: type_name::<T>(),
                pending: Vec::new(),
                lifetime: Lifetime::Singleton,
                factory: Arc::new(move |_| {
                    let value = Arc::clone(&stored);
                    Box::pin(async move { Ok(value) })
                }),
                instance: Some(Arc::new(value)),
            },
        )
        .await
    }

    /// 创建可由长寿命服务持有、但不延长容器寿命的 scope 工厂。
    pub fn scope_factory(&self) -> ScopeFactory {
        ScopeFactory {
            inner: Arc::downgrade(&self.inner),
        }
    }
    pub(crate) fn current_entry_scope(&self) -> Option<Arc<Scope>> {
        ENTRY_SCOPE
            .try_with(|scope| {
                Arc::ptr_eq(&scope.container.inner, &self.inner).then(|| Arc::clone(scope))
            })
            .ok()
            .flatten()
    }
    /// 检查实例或工厂绑定是否存在，不执行构造。
    pub fn contains<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        lock(&self.inner.state)
            .bindings
            .contains_key(&TypeId::of::<T>())
    }

    pub(crate) fn is_terminated(&self) -> bool {
        lock(&self.inner.state).terminated
    }

    /// 同步读取显式登记实例；待执行或正在执行的装饰不会泄漏旧值。
    pub fn get_instance<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        lock(&self.inner.state)
            .bindings
            .get(&TypeId::of::<T>())?
            .instance
            .as_ref()?;
        self.get_ready::<T>()
    }

    /// 同步读取成功发布的缓存；未完成的构造或装饰返回 `None`。
    pub fn get_ready<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let state = lock(&self.inner.state);
        if state.terminated {
            return None;
        }
        let service = TypeId::of::<T>();
        let binding = state.bindings.get(&service)?;
        let scope = match binding.lifetime {
            Lifetime::Bind => return None,
            Lifetime::Singleton => u64::MAX,
            Lifetime::Scoped => state.root_scope,
        };
        let key = CacheKey { service, scope };
        if state.extensions.pending_operation(key)
            || !state
                .extensions
                .instance_complete(service, binding.pending.len())
        {
            return None;
        }
        if let Some(attempt) = state.slots.get(&key) {
            return lock(&attempt.outcome)
                .as_ref()?
                .as_ref()
                .ok()?
                .downcast_ref::<Arc<T>>()
                .cloned();
        }
        if !binding.pending.is_empty() {
            return None;
        }
        binding.instance.as_ref()?.downcast_ref::<Arc<T>>().cloned()
    }

    /// 已注册服务类型数量。
    pub fn len(&self) -> usize {
        lock(&self.inner.state).bindings.len()
    }

    /// 容器尚无绑定时返回 `true`。
    pub fn is_empty(&self) -> bool {
        lock(&self.inner.state).bindings.is_empty()
    }

    /// 在根生命周期解析；`scoped` 在根也会共享，符合 Laravel 行为。
    pub async fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        let (scope, closed, resources) = {
            let state = lock(&self.inner.state);
            if state.terminated {
                return Err(container_error("container_closed", "容器已关闭"));
            }
            (
                state.root_scope,
                Arc::clone(&state.root_closed),
                state.root_resources.registrar(),
            )
        };
        self.resolver(scope, closed, resources).resolve().await
    }

    /// 关闭根 scoped 旧代，等待资源回收后才发布可解析的新代。
    pub async fn forget_scoped_instances(&self, deadline: tokio::time::Instant) -> Result<()> {
        let _reset = tokio::time::timeout_at(deadline, self.inner.reset.lock())
            .await
            .map_err(|_| container_error("scope_reset_timeout", "等待根生命周期重置超过期限"))?;
        let (closed, resources, removed) = {
            let mut state = lock(&self.inner.state);
            if state.terminated {
                return Err(container_error("container_closed", "容器已关闭"));
            }
            let closed = Arc::clone(&state.root_closed);
            closed.mark_closed();
            let old = state.root_scope;
            let resources = Arc::clone(&state.root_resources);
            let removed = close_scope(&mut state, old);
            (closed, resources, removed)
        };
        // Waker 和用户服务的 Drop 都可能同步重入容器，必须先释放内部锁。
        closed.changed.notify_waiters();
        for attempt in &removed.0 {
            attempt.changed.notify_waiters();
        }
        drop(removed);
        resources.shutdown(deadline).await?;
        let mut state = lock(&self.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        state.root_scope = state.id()?;
        state.root_closed = Arc::new(ScopeStatus::default());
        state.root_resources = Arc::new(ResourceOwner::new());
        Ok(())
    }

    /// 永久关闭根生命周期并回收其资源；之后不发布新代。
    pub async fn shutdown_root(&self, deadline: tokio::time::Instant) -> Result<()> {
        let _reset = tokio::time::timeout_at(deadline, self.inner.reset.lock())
            .await
            .map_err(|_| container_error("scope_shutdown_timeout", "等待根生命周期关闭超过期限"))?;
        self.seal_root();
        let resources = {
            let state = lock(&self.inner.state);
            Arc::clone(&state.root_resources)
        };
        resources.shutdown(deadline).await
    }

    /// 同步封闭根与当前子生命周期，供 Drop 在无运行时的场景拒绝新解析。
    pub(crate) fn seal_root(&self) {
        let (closed, resources, removed) = {
            let mut state = lock(&self.inner.state);
            state.terminated = true;
            let closed = Arc::clone(&state.root_closed);
            closed.mark_closed();
            let resources = Arc::clone(&state.root_resources);
            let root_scope = state.root_scope;
            let removed = close_scope(&mut state, root_scope);
            (closed, resources, removed)
        };
        closed.changed.notify_waiters();
        for attempt in &removed.0 {
            attempt.changed.notify_waiters();
        }
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(removed))).is_err() {
            resources.record_close_failure();
        }
        resources.seal();
    }

    /// 创建独立生命周期，不继承根或其他 scope 的 scoped 实例。
    pub fn scope(&self) -> Result<Scope> {
        let id = {
            let mut state = lock(&self.inner.state);
            if state.terminated {
                return Err(container_error("container_closed", "容器已关闭"));
            }
            state.id()?
        };
        let resources = Arc::new(ResourceOwner::new());
        let closed = Arc::new(ScopeStatus::default());
        let parent = self
            .inner
            .resources
            .as_ref()
            .map(|registrar| {
                let inner = Arc::downgrade(&self.inner);
                let on_close = Arc::clone(&closed);
                registrar
                    .track_child(Arc::clone(&resources), move || {
                        if let Some(inner) = inner.upgrade() {
                            close_scope_status(&inner, id, &on_close);
                        }
                    })
                    .map(|child_id| (registrar.clone(), child_id))
            })
            .transpose()?;
        Ok(Scope {
            container: self.clone(),
            id,
            closed,
            resources,
            parent,
        })
    }

    fn resolver(
        &self,
        scope: u64,
        closed: Arc<ScopeStatus>,
        resources: ResourceRegistrar,
    ) -> Resolver {
        Resolver {
            container: Arc::downgrade(&self.inner),
            scope,
            chain: Vec::new(),
            constructing: None,
            captive: false,
            closed,
            resources: Some(resources),
        }
    }
}

/// 手动生命周期基元；请求/队列入口将在统一 Provider 集成阶段接入。
pub struct Scope {
    container: Container,
    id: u64,
    closed: Arc<ScopeStatus>,
    resources: Arc<ResourceOwner>,
    parent: Option<(ResourceRegistrar, u64)>,
}

impl Scope {
    #[cfg(feature = "queue")]
    pub(crate) fn belongs_to(&self, container: &Container) -> bool {
        Arc::ptr_eq(&self.container.inner, &container.inner)
    }

    /// 在当前 future 安装本入口 scope；普通 `tokio::spawn` 不继承此上下文。
    pub async fn run<F: Future>(self: &Arc<Self>, future: F) -> F::Output {
        ENTRY_SCOPE.scope(Arc::clone(self), future).await
    }

    /// 在本 scope 登记受管任务，并只传递明确允许的执行元数据。
    pub fn spawn<F, Fut>(
        self: &Arc<Self>,
        metadata: super::ExecutionMetadata,
        deadline: Option<tokio::time::Instant>,
        task: F,
    ) -> Result<()>
    where
        F: FnOnce(super::Cancellation) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        if self.closed.closed.load(Ordering::Acquire) {
            return Err(container_error("scope_closed", "生命周期已关闭"));
        }
        let scope = Arc::clone(self);
        self.resources
            .registrar()
            .spawn(move |cancellation| async move {
                let execution =
                    super::ExecutionContext::new(metadata, deadline, cancellation.clone());
                scope
                    .run(super::with_execution_context(execution, async move {
                        task(cancellation).await;
                    }))
                    .await;
            })
    }

    /// 在本生命周期解析服务。
    pub async fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.container
            .resolver(
                self.id,
                Arc::clone(&self.closed),
                self.resources.registrar(),
            )
            .resolve()
            .await
    }

    /// 封闭解析并显式等待本 scope 的资源回收。
    pub async fn shutdown(&self, deadline: tokio::time::Instant) -> Result<()> {
        self.close_cache();
        let result = self.resources.shutdown(deadline).await;
        if let Some((parent, child_id)) = &self.parent {
            parent.finish_child(*child_id, result.is_err());
        }
        result
    }

    /// 关闭本生命周期并启动有界资源回收；需确认完成时使用 `shutdown`。
    pub fn close(&self) {
        if !self.close_cache() {
            return;
        }
        self.resources.cancel_tasks();
        let timeout = self
            .container
            .inner
            .config
            .as_ref()
            .and_then(|config| config.section::<ServerConfig>("server").ok())
            .unwrap_or_default()
            .shutdown_timeout_secs;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout);
        if let Some((parent, child_id)) = &self.parent {
            if let Err(error) = parent.request_child_cleanup(*child_id, deadline) {
                tracing::error!(code = error.code(), "scope cleanup submission failed");
            }
            // 父所有者保留未完成的子资源；提交失败由 App.shutdown 有界回收。
            return;
        }
        let resources = Arc::clone(&self.resources);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let result = resources.shutdown(deadline).await;
                if let Err(error) = result {
                    tracing::error!(code = error.code(), "scope resource cleanup failed");
                }
            });
        } else {
            tracing::error!("scope cleanup needs a runtime or explicit owner shutdown");
        }
    }

    fn close_cache(&self) -> bool {
        close_scope_status(&self.container.inner, self.id, &self.closed)
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.close();
    }
}

fn close_scope_status(inner: &Arc<Inner>, id: u64, closed: &Arc<ScopeStatus>) -> bool {
    let mut state = lock(&inner.state);
    if closed.closed.swap(true, Ordering::AcqRel) {
        return false;
    }
    closed.mark_closed();
    let removed = close_scope(&mut state, id);
    drop(state);
    closed.changed.notify_waiters();
    for attempt in &removed.0 {
        attempt.changed.notify_waiters();
    }
    drop(removed);
    true
}

fn close_scope(state: &mut State, scope: u64) -> (Vec<Arc<Attempt>>, Vec<Extender>) {
    let keys: Vec<_> = state
        .slots
        .keys()
        .filter(|key| key.scope == scope)
        .copied()
        .collect();
    let removed: Vec<_> = keys
        .into_iter()
        .filter_map(|key| state.slots.remove(&key))
        .collect();
    for attempt in &removed {
        let mut outcome = lock(&attempt.outcome);
        if outcome.is_none() {
            *outcome = Some(Err(Failure {
                code: "scope_closed",
                message: "生命周期已关闭".into(),
                source: None,
            }));
        }
    }
    let extenders = state.extensions.discard_scope(scope);
    (removed, extenders)
}

struct ActiveFactory(Arc<AtomicBool>);

impl Drop for ActiveFactory {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// 工厂收到的解析上下文；仅活跃构造帧参与环检测，已结束的工厂不会污染延迟解析。
#[derive(Clone)]
pub struct Resolver {
    container: Weak<Inner>,
    scope: u64,
    chain: Vec<(TypeId, &'static str, Arc<AtomicBool>)>,
    constructing: Option<(u64, Arc<AtomicBool>)>,
    captive: bool,
    closed: Arc<ScopeStatus>,
    resources: Option<ResourceRegistrar>,
}

impl Resolver {
    // 长寿命的模块管理句柄只保留既有弱容器；事件使用当前根上下文，不续用已关闭的旧scoped代。
    pub(crate) async fn resolve_root<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.container()?.resolve::<T>().await
    }

    /// 取得同一容器的弱 scope 工厂，用于 scheduler 等长寿命服务创建独立入口。
    pub fn scope_factory(&self) -> ScopeFactory {
        ScopeFactory {
            inner: self.container.clone(),
        }
    }
    /// 检查同一容器是否声明服务，不执行工厂；容器已释放时返回错误。
    pub fn is_bound<T: ?Sized + Send + Sync + 'static>(&self) -> Result<bool> {
        Ok(self.container()?.contains::<T>())
    }

    /// 从同一应用的已发布配置视图读取区段；boot overlay 后首次解析可见新版本。
    pub fn section<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> std::result::Result<T, ConfigError> {
        let container = self
            .container()
            .map_err(|_| ConfigError::Invalid("服务容器已关闭，无法读取配置".into()))?;
        container
            .inner
            .config
            .as_ref()
            .ok_or_else(|| ConfigError::Invalid("服务容器未配置配置仓储".into()))?
            .section(key)
    }

    /// 当前服务生命周期的资源登记句柄；singleton 归应用，scoped 归请求或根 scope。
    pub fn resources(&self) -> Result<ResourceRegistrar> {
        if self.closed.closed.load(Ordering::Acquire) {
            return Err(container_error("scope_closed", "生命周期已关闭"));
        }
        self.resources
            .clone()
            .ok_or_else(|| container_error("resources_unavailable", "服务容器未配置资源所有者"))
    }

    fn container(&self) -> Result<Container> {
        let container = self
            .container
            .upgrade()
            .map(|inner| Container { inner })
            .ok_or_else(|| container_error("container_closed", "容器已释放"))?;
        if container.is_terminated() {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        Ok(container)
    }

    /// 解析具体类型或 `dyn Trait`；缺失、环和构造失败返回错误。
    pub fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> BoxFuture<'_, Result<Arc<T>>> {
        Box::pin(async move {
            let value = self
                .resolve_value(TypeId::of::<T>(), type_name::<T>())
                .await?;
            value.downcast_ref::<Arc<T>>().cloned().ok_or_else(|| {
                container_error(
                    "service_type_mismatch",
                    format!("服务 `{}` 内部类型不匹配", type_name::<T>()),
                )
            })
        })
    }

    fn resolve_value(&self, key: TypeId, name: &'static str) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let container = self.container()?;
            if self.closed.closed.load(Ordering::Acquire) {
                return Err(container_error("scope_closed", "生命周期已关闭"));
            }
            if self
                .chain
                .iter()
                .any(|(id, _, active)| *id == key && active.load(Ordering::Acquire))
            {
                let mut chain: Vec<_> = self
                    .chain
                    .iter()
                    .filter(|(_, _, active)| active.load(Ordering::Acquire))
                    .map(|(_, name, _)| *name)
                    .collect();
                chain.push(name);
                return Err(container_error("service_cycle", chain.join(" → ")));
            }
            let binding = {
                let state = lock(&container.inner.state);
                state.bindings.get(&key).cloned().ok_or_else(|| {
                    container_error(
                        "service_not_bound",
                        format!(
                            "未绑定服务：{}",
                            self.chain
                                .iter()
                                .filter(|(_, _, active)| active.load(Ordering::Acquire))
                                .map(|(_, n, _)| *n)
                                .chain(std::iter::once(name))
                                .collect::<Vec<_>>()
                                .join(" → ")
                        ),
                    )
                })?
            };
            if !binding.pending.is_empty()
                && let Err(error) = self
                    .initialize_instance(&container.inner, key, &binding)
                    .await
            {
                if error.code() == "binding_replaced" {
                    return self.resolve_value(key, name).await;
                }
                return Err(error);
            }
            if binding.lifetime == Lifetime::Scoped && self.captive {
                let stale = lock(&container.inner.state)
                    .bindings
                    .get(&key)
                    .map(|current| current.generation)
                    != Some(binding.generation);
                if stale {
                    return self.resolve_value(key, name).await;
                }
                return Err(container_error(
                    "captive_scope_dependency",
                    format!("singleton 不能捕获 scoped 服务 `{name}`"),
                ));
            }
            let mut child = self.clone();
            child
                .chain
                .retain(|(_, _, active)| active.load(Ordering::Acquire));
            let activity = Arc::new(AtomicBool::new(true));
            let _activity = ActiveFactory(Arc::clone(&activity));
            child.chain.push((key, name, Arc::clone(&activity)));
            if binding.lifetime == Lifetime::Singleton {
                child.captive = true;
                child.scope = u64::MAX;
                child.closed = Arc::new(ScopeStatus::default());
                child.resources = container.inner.resources.clone();
            }
            let value = if binding.lifetime == Lifetime::Bind {
                match self.transient(&binding, child, key, name, activity).await? {
                    Some(value) => value,
                    None => return self.resolve_value(key, name).await,
                }
            } else {
                match self.cached(&binding, child, key, name, activity).await? {
                    Some(value) => value,
                    None => return self.resolve_value(key, name).await,
                }
            };
            if self.closed.closed.load(Ordering::Acquire) {
                return Err(container_error("scope_closed", "生命周期已关闭"));
            }
            Ok(value)
        })
    }

    async fn cached(
        &self,
        binding: &Binding,
        mut child: Self,
        service: TypeId,
        name: &'static str,
        activity: Arc<AtomicBool>,
    ) -> Result<Option<Value>> {
        let container = self.container()?;
        // singleton 不能和 root scoped 共用一个缓存键。
        let key = CacheKey {
            service,
            scope: if binding.lifetime == Lifetime::Singleton {
                u64::MAX
            } else {
                self.scope
            },
        };
        let Some((attempt, owner, _edge)) = self
            .admit_cached(&container.inner, key, name, binding.generation)
            .await?
        else {
            return Ok(None);
        };
        if !owner {
            return tokio::select! {
                result = attempt.wait() => result.map(Some),
                () = self.closed.wait_closed() => Err(container_error("scope_closed", "生命周期已关闭")),
            };
        }
        let mut guard = Construction {
            inner: Arc::clone(&container.inner),
            key,
            attempt: Arc::clone(&attempt),
            activity,
            finished: false,
        };
        child.constructing = child
            .chain
            .last()
            .map(|(_, _, active)| (attempt.id, Arc::clone(active)));
        let outcome = tokio::select! {
            result = child.construct_value(binding, key) => result,
            closed = attempt.wait() => return closed.map(Some),
            () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
        };
        match outcome {
            Ok(value) => {
                guard.finish(Ok(Arc::clone(&value)));
                let value = attempt.wait().await?;
                if !binding.pending.is_empty() {
                    self.notify_rebinding(&container.inner, service, Arc::clone(&value))
                        .await?;
                }
                Ok(Some(value))
            }
            Err(error) => {
                let failure = Failure::initialization(name, error);
                guard.finish(Err(failure));
                attempt.wait().await.map(Some)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // 测试中的失败直接终止断言，不作为库生产路径的错误处理。
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    trait Greeting: Send + Sync {
        fn greet(&self) -> &'static str;
    }

    #[tokio::test]
    async fn stale_binding_snapshot_cannot_enter_replacement_slot() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
        let container = builder.build();
        let old = lock(&container.inner.state)
            .bindings
            .get(&TypeId::of::<usize>())
            .cloned()
            .unwrap();
        let resolver = {
            let state = lock(&container.inner.state);
            container.resolver(
                state.root_scope,
                Arc::clone(&state.root_closed),
                state.root_resources.registrar(),
            )
        };
        container
            .singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
            .await?;
        let stale = resolver
            .cached(
                &old,
                resolver.clone(),
                TypeId::of::<usize>(),
                type_name::<usize>(),
                Arc::new(AtomicBool::new(true)),
            )
            .await?;
        assert!(stale.is_none());
        assert_eq!(*container.resolve::<usize>().await?, 2);
        assert_eq!(*container.get_ready::<usize>().unwrap(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn old_attempt_waiters_finish_without_filling_new_slot() -> Result<()> {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut builder = ContainerBuilder::new();
        let notice = Arc::clone(&started);
        let finish = Arc::clone(&release);
        builder.singleton::<usize, _>(move |_| {
            let notice = Arc::clone(&notice);
            let finish = Arc::clone(&finish);
            Box::pin(async move {
                notice.notify_one();
                finish.notified().await;
                Ok(Arc::new(1))
            })
        });
        let container = builder.build();
        let owner = container.clone();
        let task = tokio::spawn(async move { owner.resolve::<usize>().await });
        started.notified().await;
        let mut waiter = Box::pin(container.resolve::<usize>());
        tokio::select! { biased; _ = &mut waiter => panic!("旧构造尚未完成"), () = tokio::task::yield_now() => {} }
        container
            .singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
            .await?;
        let new = container.resolve::<usize>().await?;
        assert_eq!(*new, 2);
        release.notify_one();
        assert_eq!(*task.await.unwrap()?, 1);
        assert_eq!(*waiter.await?, 1);
        assert!(Arc::ptr_eq(&new, &container.resolve::<usize>().await?));
        Ok(())
    }

    #[tokio::test]
    async fn rebinding_releases_operation_waiter_without_waiting_for_old_extender() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        builder.instance(Arc::new(1usize));
        let container = builder.build();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let notice = Arc::clone(&started);
        let blocked = Arc::clone(&release);
        let old = container.clone();
        let old_task = tokio::spawn(async move {
            old.extend::<usize, _>(move |value, _| {
                let notice = Arc::clone(&notice);
                let blocked = Arc::clone(&blocked);
                Box::pin(async move {
                    notice.notify_one();
                    blocked.notified().await;
                    Ok(Arc::new(*value + 100))
                })
            })
            .await
        });
        started.notified().await;
        let mut waiter = Box::pin(
            container
                .extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 10)) })),
        );
        tokio::select! { biased; _ = &mut waiter => panic!("旧 operation 仍在运行"), () = tokio::task::yield_now() => {} }
        container.instance(Arc::new(2usize)).await?;
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .map_err(|_| Error::custom("operation_waiter_stuck", "新定义等待者未在期限内完成"))??;
        assert_eq!(*container.resolve::<usize>().await?, 12);
        assert!(!old_task.is_finished());
        old_task.abort();
        assert!(old_task.await.unwrap_err().is_cancelled());
        assert_eq!(*container.resolve::<usize>().await?, 12);
        Ok(())
    }

    #[tokio::test]
    async fn live_rebinding_keeps_new_definition_after_callback_error() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        builder.instance(Arc::new(1usize));
        let container = builder.build();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let current = container
            .rebinding::<usize, _>(move |_, value| {
                let count = Arc::clone(&count);
                Box::pin(async move {
                    count.fetch_add(*value, Ordering::SeqCst);
                    Err(Error::custom("rebinding_failed", "回调拒绝"))
                })
            })
            .await?;
        assert_eq!(current.as_deref(), Some(&1));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let error = container.instance(Arc::new(2usize)).await.unwrap_err();
        assert_eq!(error.code(), "rebinding_failed");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(*container.resolve::<usize>().await?, 2);
        Ok(())
    }

    #[tokio::test]
    async fn live_rebinding_without_listener_does_not_eagerly_make_factory() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        builder.instance(Arc::new(1usize));
        let container = builder.build();
        assert_eq!(*container.resolve::<usize>().await?, 1);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        container
            .singleton::<usize, _>(move |_| {
                let count = Arc::clone(&count);
                Box::pin(async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Err(Error::custom("new_factory_failed", "新工厂失败"))
                })
            })
            .await?;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            container.resolve::<usize>().await.unwrap_err().code(),
            "service_initialization_failed"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn rebinding_registration_makes_bound_singleton_and_transient() -> Result<()> {
        let singleton_calls = Arc::new(AtomicUsize::new(0));
        let transient_calls = Arc::new(AtomicUsize::new(0));
        let mut builder = ContainerBuilder::new();
        let first = Arc::clone(&singleton_calls);
        builder.singleton::<usize, _>(move |_| {
            let first = Arc::clone(&first);
            Box::pin(async move { Ok(Arc::new(first.fetch_add(1, Ordering::SeqCst) + 1)) })
        });
        let second = Arc::clone(&transient_calls);
        builder.bind::<String, _>(move |_| {
            let second = Arc::clone(&second);
            Box::pin(async move {
                Ok(Arc::new(format!(
                    "{}",
                    second.fetch_add(1, Ordering::SeqCst) + 1
                )))
            })
        });
        let container = builder.build();
        let singleton = container
            .rebinding::<usize, _>(|_, _| Box::pin(async { Ok(()) }))
            .await?;
        let transient = container
            .rebinding::<String, _>(|_, _| Box::pin(async { Ok(()) }))
            .await?;
        assert_eq!(singleton.as_deref(), Some(&1));
        assert_eq!(transient.as_deref().map(String::as_str), Some("1"));
        assert_eq!(singleton_calls.load(Ordering::SeqCst), 1);
        assert_eq!(transient_calls.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(
            &singleton.unwrap(),
            &container.resolve::<usize>().await?
        ));
        assert_eq!(container.resolve::<String>().await?.as_str(), "2");
        Ok(())
    }

    #[tokio::test]
    async fn rebinding_registration_unbound_and_failed_make_keep_listener() -> Result<()> {
        let container = ContainerBuilder::new().build();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        assert!(
            container
                .rebinding::<u8, _>(move |_, _| {
                    let count = Arc::clone(&count);
                    Box::pin(async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })
                .await?
                .is_none()
        );
        container.instance(Arc::new(1u8)).await?;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        container.resolve::<u8>().await?;
        container.instance(Arc::new(2u8)).await?;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let attempts = Arc::new(AtomicUsize::new(0));
        let mut builder = ContainerBuilder::new();
        let tries = Arc::clone(&attempts);
        builder.singleton::<usize, _>(move |_| {
            let tries = Arc::clone(&tries);
            Box::pin(async move {
                if tries.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(Error::custom("first_make_failed", "首次构造失败"))
                } else {
                    Ok(Arc::new(3))
                }
            })
        });
        let container = builder.build();
        let notifications = Arc::new(AtomicUsize::new(0));
        let notified = Arc::clone(&notifications);
        let error = container
            .rebinding::<usize, _>(move |_, _| {
                let notified = Arc::clone(&notified);
                Box::pin(async move {
                    notified.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), "service_initialization_failed");
        assert_eq!(notifications.load(Ordering::SeqCst), 0);
        assert_eq!(*container.resolve::<usize>().await?, 3);
        container.instance(Arc::new(4usize)).await?;
        assert_eq!(notifications.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn live_scoped_rebinding_invalidates_root_and_child_caches() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
        let container = builder.build();
        let scope = container.scope()?;
        let old_root = container.resolve::<usize>().await?;
        let old_child = scope.resolve::<usize>().await?;
        container
            .scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
            .await?;
        assert_eq!(*old_root, 1);
        assert_eq!(*old_child, 1);
        assert_eq!(*container.resolve::<usize>().await?, 2);
        assert_eq!(*scope.resolve::<usize>().await?, 2);
        scope
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await?;
        container
            .shutdown_root(tokio::time::Instant::now() + Duration::from_secs(1))
            .await?;
        assert_eq!(
            container
                .instance(Arc::new(3usize))
                .await
                .unwrap_err()
                .code(),
            "container_closed"
        );
        Ok(())
    }
    struct Hello;
    impl Greeting for Hello {
        fn greet(&self) -> &'static str {
            "你好"
        }
    }

    #[tokio::test]
    async fn scope_factory_is_weak_and_creates_distinct_lifecycles() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        builder.scoped::<usize, _>({
            let next = Arc::new(AtomicUsize::new(0));
            move |_| {
                let next = Arc::clone(&next);
                Box::pin(async move { Ok(Arc::new(next.fetch_add(1, Ordering::SeqCst) + 1)) })
            }
        });
        let container = builder.build();
        let factory = container.scope_factory();
        let first = factory.scope()?;
        let second = factory.scope()?;
        assert_eq!(*first.resolve::<usize>().await?, 1);
        assert_eq!(*second.resolve::<usize>().await?, 2);
        first
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await?;
        second
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await?;
        drop(first);
        drop(second);
        drop(container);
        assert_eq!(
            factory.scope().err().map(|error| error.code().to_owned()),
            Some("container_closed".to_owned())
        );
        Ok(())
    }

    #[tokio::test]
    async fn synchronous_ready_read_uses_only_constructed_singleton() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        builder.singleton::<String, _>(|_| Box::pin(async { Ok(Arc::new("ready".to_owned())) }));
        let container = builder.build();
        assert!(container.get_ready::<String>().is_none());
        let resolved = container.resolve::<String>().await?;
        let ready = container
            .get_ready::<String>()
            .ok_or_else(|| Error::custom("ready_service_missing", "已解析单例不可同步读取"))?;
        assert!(Arc::ptr_eq(&resolved, &ready));
        Ok(())
    }

    #[tokio::test]
    async fn sealed_container_does_not_reissue_registered_instance() -> Result<()> {
        let mut builder = ContainerBuilder::new();
        builder.instance(Arc::new(7usize));
        let container = builder.build();
        let held = container
            .get_instance::<usize>()
            .ok_or_else(|| Error::custom("registered_instance_missing", "已登记实例缺失"))?;
        container.seal_root();
        assert_eq!(*held, 7);
        assert!(container.get_instance::<usize>().is_none());
        assert!(container.get_ready::<usize>().is_none());
        assert_eq!(
            container
                .resolve::<usize>()
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("container_closed".to_owned())
        );
        assert_eq!(
            container.scope().err().map(|error| error.code().to_owned()),
            Some("container_closed".to_owned())
        );
        container
            .shutdown_root(tokio::time::Instant::now() + Duration::from_secs(1))
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn trait_binding_override_and_transient_lifetime() {
        let mut builder = ContainerBuilder::new();
        builder
            .instance::<usize>(Arc::new(1))
            .instance::<usize>(Arc::new(2));
        builder.bind::<String, _>(|_| Box::pin(async { Ok(Arc::new("业务".into())) }));
        builder.instance::<dyn Greeting>(Arc::new(Hello));
        let container = builder.build();
        assert_eq!(*container.resolve::<usize>().await.unwrap(), 2);
        assert_eq!(
            container.resolve::<dyn Greeting>().await.unwrap().greet(),
            "你好"
        );
        let first = container.resolve::<String>().await.unwrap();
        assert!(!Arc::ptr_eq(
            &first,
            &container.resolve::<String>().await.unwrap()
        ));
        assert_eq!(
            container.resolve::<u8>().await.unwrap_err().code(),
            "service_not_bound"
        );
    }

    #[tokio::test]
    async fn singleton_singleflight_and_app_isolation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = ContainerBuilder::new();
        let count = Arc::clone(&calls);
        builder.singleton::<usize, _>(move |_| {
            let count = Arc::clone(&count);
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(Arc::new(7))
            })
        });
        let container = builder.build();
        let (a, b) = tokio::join!(container.resolve::<usize>(), container.resolve::<usize>());
        assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mut other = ContainerBuilder::new();
        other.instance::<usize>(Arc::new(7));
        assert!(!Arc::ptr_eq(
            &container.resolve::<usize>().await.unwrap(),
            &other.build().resolve::<usize>().await.unwrap()
        ));
    }

    #[tokio::test]
    async fn root_scoped_reset_and_child_scope_isolation() {
        let mut builder = ContainerBuilder::new();
        builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
        let container = builder.build();
        let old = container.resolve::<usize>().await.unwrap();
        assert!(Arc::ptr_eq(
            &old,
            &container.resolve::<usize>().await.unwrap()
        ));
        let scope = container.scope().unwrap();
        let child = scope.resolve::<usize>().await.unwrap();
        assert!(!Arc::ptr_eq(&old, &child));
        assert!(Arc::ptr_eq(
            &child,
            &scope.resolve::<usize>().await.unwrap()
        ));
        container
            .forget_scoped_instances(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(!Arc::ptr_eq(
            &old,
            &container.resolve::<usize>().await.unwrap()
        ));
        assert!(Arc::ptr_eq(
            &child,
            &scope.resolve::<usize>().await.unwrap()
        ));
        scope.close();
        assert_eq!(
            scope.resolve::<usize>().await.unwrap_err().code(),
            "scope_closed"
        );
    }

    #[tokio::test]
    async fn nested_cycle_and_captive_dependency_fail() {
        let mut builder = ContainerBuilder::new();
        builder.singleton::<u8, _>(|r| {
            Box::pin(async move {
                r.resolve::<u16>().await?;
                Ok(Arc::new(1))
            })
        });
        builder.singleton::<u16, _>(|r| {
            Box::pin(async move {
                r.resolve::<u8>().await?;
                Ok(Arc::new(1))
            })
        });
        let error = builder.build().resolve::<u8>().await.unwrap_err();
        assert_eq!(error.code(), "service_cycle");
        let mut builder = ContainerBuilder::new();
        builder.scoped::<u8, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
        builder.bind::<u16, _>(|r| {
            Box::pin(async move {
                r.resolve::<u8>().await?;
                Ok(Arc::new(1))
            })
        });
        builder.singleton::<u32, _>(|r| {
            Box::pin(async move {
                r.resolve::<u16>().await?;
                Ok(Arc::new(1))
            })
        });
        assert_eq!(
            builder.build().resolve::<u32>().await.unwrap_err().code(),
            "captive_scope_dependency"
        );
    }

    #[tokio::test]
    async fn concurrent_root_cycle_has_bounded_error() {
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let mut builder = ContainerBuilder::new();
        let first = Arc::clone(&barrier);
        builder.singleton::<u8, _>(move |r| {
            let barrier = Arc::clone(&first);
            Box::pin(async move {
                barrier.wait().await;
                r.resolve::<u16>().await?;
                Ok(Arc::new(1))
            })
        });
        builder.singleton::<u16, _>(move |r| {
            let barrier = Arc::clone(&barrier);
            Box::pin(async move {
                barrier.wait().await;
                r.resolve::<u8>().await?;
                Ok(Arc::new(1))
            })
        });
        let container = builder.build();
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(container.resolve::<u8>(), container.resolve::<u16>())
        })
        .await
        .unwrap();
        assert_eq!(result.0.unwrap_err().code(), "service_cycle");
        let error = result.1.unwrap_err();
        assert_eq!(error.code(), "service_cycle");
        assert!(error.to_string().contains("u8"));
        assert!(error.to_string().contains("u16"));
    }

    #[tokio::test]
    async fn cancelled_owner_releases_waiters_and_allows_retry() {
        let started = Arc::new(Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = ContainerBuilder::new();
        let count = Arc::clone(&calls);
        let notice = Arc::clone(&started);
        builder.singleton::<usize, _>(move |_| {
            let count = Arc::clone(&count);
            let notice = Arc::clone(&notice);
            Box::pin(async move {
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    notice.notify_one();
                    std::future::pending::<()>().await;
                }
                Ok(Arc::new(3))
            })
        });
        let container = builder.build();
        let owner = container.clone();
        let task = tokio::spawn(async move { owner.resolve::<usize>().await });
        started.notified().await;
        let mut waiter = Box::pin(container.resolve::<usize>());
        tokio::select! { _ = &mut waiter => panic!("构造尚未完成"), () = tokio::task::yield_now() => {} }
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), waiter)
                .await
                .unwrap()
                .unwrap_err()
                .code(),
            "service_initialization_cancelled"
        );
        assert_eq!(*container.resolve::<usize>().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn initialization_source_is_retained_without_debug_or_response_leak() {
        use std::error::Error as _;
        let mut builder = ContainerBuilder::new();
        builder.singleton::<usize, _>(|_| {
            Box::pin(async { Err(Error::custom("backend_failure", "SECRET_SENTINEL_PASSWORD")) })
        });
        let container = builder.build();
        let error = container.resolve::<usize>().await.unwrap_err();
        assert_eq!(error.code(), "service_initialization_failed");
        assert_eq!(error.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!format!("{error:?}").contains("SECRET_SENTINEL"));
        assert!(!error.to_string().contains("SECRET_SENTINEL"));
        assert!(
            !serde_json::to_string(&error.to_api_response())
                .unwrap()
                .contains("SECRET_SENTINEL")
        );
        assert!(
            error
                .source()
                .unwrap()
                .to_string()
                .contains("SECRET_SENTINEL")
        );
        assert!(lock(&container.inner.state).slots.is_empty());
    }

    #[tokio::test]
    async fn nested_missing_service_retains_code_and_safe_chain() {
        let mut builder = ContainerBuilder::new();
        builder.singleton::<usize, _>(|r| {
            Box::pin(async move {
                r.resolve::<String>().await?;
                Ok(Arc::new(1))
            })
        });
        let error = builder.build().resolve::<usize>().await.unwrap_err();
        assert_eq!(error.code(), "service_not_bound");
        assert!(error.to_string().contains("usize"));
        assert!(error.to_string().contains("String"));
    }

    #[tokio::test]
    async fn reset_closes_pending_old_generation_and_preserves_singleton() {
        let started = Arc::new(Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut builder = ContainerBuilder::new();
        builder.instance::<String>(Arc::new("共享".into()));
        let notice = Arc::clone(&started);
        builder.scoped::<usize, _>(move |_| {
            let notice = Arc::clone(&notice);
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    notice.notify_one();
                    std::future::pending::<()>().await;
                }
                Ok(Arc::new(8))
            })
        });
        let container = builder.build();
        let singleton = container.resolve::<String>().await.unwrap();
        let owner = container.clone();
        let task = tokio::spawn(async move { owner.resolve::<usize>().await });
        started.notified().await;
        container
            .forget_scoped_instances(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code(),
            "scope_closed"
        );
        assert_eq!(*container.resolve::<usize>().await.unwrap(), 8);
        assert!(Arc::ptr_eq(
            &singleton,
            &container.resolve::<String>().await.unwrap()
        ));
    }

    #[tokio::test]
    async fn dropped_scope_releases_cache_without_lifetime_tombstones() {
        let mut builder = ContainerBuilder::new();
        builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
        let container = builder.build();
        for _ in 0..32 {
            let scope = container.scope().unwrap();
            scope.resolve::<usize>().await.unwrap();
        }
        assert!(lock(&container.inner.state).slots.is_empty());
    }

    #[tokio::test]
    async fn saved_resolver_does_not_keep_finished_factory_in_cycle_chain() {
        struct Lazy(Resolver);
        let mut builder = ContainerBuilder::new();
        builder.bind::<Lazy, _>(|r| Box::pin(async move { Ok(Arc::new(Lazy(r))) }));
        let container = builder.build();
        let first = container.resolve::<Lazy>().await.unwrap();
        let second = first.0.resolve::<Lazy>().await.unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[tokio::test]
    async fn closing_scope_releases_singleton_waiter_without_cancelling_other_scope() {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut builder = ContainerBuilder::new();
        let notice = Arc::clone(&started);
        let finish = Arc::clone(&release);
        builder.singleton::<usize, _>(move |_| {
            let notice = Arc::clone(&notice);
            let finish = Arc::clone(&finish);
            Box::pin(async move {
                notice.notify_one();
                finish.notified().await;
                Ok(Arc::new(1))
            })
        });
        let container = builder.build();
        let root = container.clone();
        let owner = tokio::spawn(async move { root.resolve::<usize>().await });
        started.notified().await;
        let scope = container.scope().unwrap();
        let mut waiter = Box::pin(scope.resolve::<usize>());
        tokio::select! { _ = &mut waiter => panic!("根构造尚未完成"), () = tokio::task::yield_now() => {} }
        scope.close();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), waiter)
                .await
                .unwrap()
                .unwrap_err()
                .code(),
            "scope_closed"
        );
        release.notify_one();
        assert_eq!(*owner.await.unwrap().unwrap(), 1);
    }

    #[tokio::test]
    async fn closing_singleton_owner_scope_cancels_inflight_construction() {
        let started = Arc::new(Notify::new());
        let mut builder = ContainerBuilder::new();
        let notice = Arc::clone(&started);
        builder.singleton::<usize, _>(move |_| {
            let notice = Arc::clone(&notice);
            Box::pin(async move {
                notice.notify_one();
                std::future::pending::<Result<Arc<usize>>>().await
            })
        });
        let container = builder.build();
        let scope = Arc::new(container.scope().unwrap());
        let request_scope = Arc::clone(&scope);
        let owner = tokio::spawn(async move { request_scope.resolve::<usize>().await });
        started.notified().await;
        scope.close();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), owner)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code(),
            "scope_closed"
        );
        assert!(lock(&container.inner.state).slots.is_empty());
    }

    #[tokio::test]
    async fn scoped_value_drop_can_reenter_container_after_cache_release() {
        struct Reentrant(Container, Arc<AtomicBool>);
        impl Drop for Reentrant {
            fn drop(&mut self) {
                let _scope = self.0.scope().unwrap();
                self.1.store(true, Ordering::Release);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&dropped);
        let mut builder = ContainerBuilder::new();
        builder.scoped::<Reentrant, _>(move |r| {
            let flag = Arc::clone(&flag);
            Box::pin(async move { Ok(Arc::new(Reentrant(r.container()?, flag))) })
        });
        let container = builder.build();
        container.resolve::<Reentrant>().await.unwrap();
        container
            .forget_scoped_instances(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn root_reset_waits_for_old_resource_before_new_generation() {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut builder = ContainerBuilder::new();
        builder.scoped::<usize, _>({
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            move |resolver| {
                let entered = Arc::clone(&entered);
                let release = Arc::clone(&release);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            entered.notify_one();
                            release.notified().await;
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(1))
                })
            }
        });
        let container = builder.build();
        container.resolve::<usize>().await.unwrap();
        let copy = container.clone();
        let reset = tokio::spawn(async move {
            copy.forget_scoped_instances(tokio::time::Instant::now() + Duration::from_secs(1))
                .await
        });
        entered.notified().await;
        assert_eq!(
            container.resolve::<usize>().await.unwrap_err().code(),
            "scope_closed"
        );
        release.notify_one();
        reset.await.unwrap().unwrap();
        assert_eq!(*container.resolve::<usize>().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn app_shutdown_joins_scope_cleanup_started_by_close() {
        let app_owner = ResourceOwner::new();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut builder = ContainerBuilder::new();
        builder.context(
            ConfigRepository::new(figment::Figment::new()),
            app_owner.registrar(),
        );
        builder.scoped::<usize, _>({
            let calls = Arc::clone(&calls);
            move |resolver| {
                let calls = Arc::clone(&calls);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(1))
                })
            }
        });
        let container = builder.build();
        let scope = container.scope().unwrap();
        scope.resolve::<usize>().await.unwrap();
        scope.close();
        app_owner
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_root_reset_does_not_publish_new_generation() {
        let mut builder = ContainerBuilder::new();
        builder.scoped::<usize, _>(|resolver| {
            Box::pin(async move {
                resolver.resources()?.on_shutdown(|| {
                    Box::pin(async { std::future::pending::<Result<()>>().await })
                })?;
                Ok(Arc::new(1))
            })
        });
        let container = builder.build();
        container.resolve::<usize>().await.unwrap();
        let error = container
            .forget_scoped_instances(tokio::time::Instant::now() + Duration::from_millis(10))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "resource_shutdown_timeout");
        assert_eq!(
            container.resolve::<usize>().await.unwrap_err().code(),
            "scope_closed"
        );
    }

    #[tokio::test]
    async fn queued_root_reset_obeys_its_own_deadline() {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut builder = ContainerBuilder::new();
        builder.scoped::<usize, _>({
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            move |resolver| {
                let entered = Arc::clone(&entered);
                let release = Arc::clone(&release);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            entered.notify_one();
                            release.notified().await;
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(1))
                })
            }
        });
        let container = builder.build();
        container.resolve::<usize>().await.unwrap();
        let first = container.clone();
        let first_reset = tokio::spawn(async move {
            first
                .forget_scoped_instances(tokio::time::Instant::now() + Duration::from_secs(1))
                .await
        });
        entered.notified().await;
        let error = container
            .forget_scoped_instances(tokio::time::Instant::now() + Duration::from_millis(10))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "scope_reset_timeout");
        release.notify_one();
        first_reset.await.unwrap().unwrap();
        assert_eq!(*container.resolve::<usize>().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn app_shutdown_reclaims_child_when_scope_closes_after_parent_seals() {
        let app_owner = Arc::new(ResourceOwner::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut builder = ContainerBuilder::new();
        builder.context(
            ConfigRepository::new(figment::Figment::new()),
            app_owner.registrar(),
        );
        builder.scoped::<usize, _>({
            let calls = Arc::clone(&calls);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            move |resolver| {
                let calls = Arc::clone(&calls);
                let entered = Arc::clone(&entered);
                let release = Arc::clone(&release);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            entered.notify_one();
                            release.notified().await;
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(1))
                })
            }
        });
        let container = builder.build();
        let scope = container.scope().unwrap();
        scope.resolve::<usize>().await.unwrap();
        let owner = Arc::clone(&app_owner);
        let shutdown = tokio::spawn(async move {
            owner
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
                .await
        });
        entered.notified().await;
        scope.close();
        release.notify_one();
        shutdown.await.unwrap().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn child_cleanup_failure_reaches_app_shutdown() {
        let app_owner = ResourceOwner::new();
        let mut builder = ContainerBuilder::new();
        builder.context(
            ConfigRepository::new(figment::Figment::new()),
            app_owner.registrar(),
        );
        builder.scoped::<usize, _>(|resolver| {
            Box::pin(async move {
                resolver.resources()?.on_shutdown(|| {
                    Box::pin(async { Err(Error::custom("child_failed", "child failed")) })
                })?;
                Ok(Arc::new(1))
            })
        });
        let container = builder.build();
        let scope = container.scope().unwrap();
        scope.resolve::<usize>().await.unwrap();
        scope.close();
        let error = app_owner
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "resource_shutdown_failed");
    }

    #[tokio::test]
    async fn app_shutdown_closes_retained_scope_before_reclaiming_resources() {
        let app_owner = ResourceOwner::new();
        let closed = Arc::new(AtomicBool::new(false));
        let mut builder = ContainerBuilder::new();
        builder.context(
            ConfigRepository::new(figment::Figment::new()),
            app_owner.registrar(),
        );
        builder.scoped::<usize, _>({
            let closed = Arc::clone(&closed);
            move |resolver| {
                let closed = Arc::clone(&closed);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            closed.store(true, Ordering::SeqCst);
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(1))
                })
            }
        });
        let container = builder.build();
        let scope = container.scope().unwrap();
        scope.resolve::<usize>().await.unwrap();
        app_owner
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert!(closed.load(Ordering::SeqCst));
        assert_eq!(
            scope.resolve::<usize>().await.unwrap_err().code(),
            "scope_closed"
        );
    }

    #[tokio::test]
    async fn app_shutdown_seals_all_scopes_before_waiting_for_background_tasks() {
        let app_owner = Arc::new(ResourceOwner::new());
        let cancelled = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        app_owner
            .registrar()
            .spawn({
                let cancelled = Arc::clone(&cancelled);
                let release = Arc::clone(&release);
                move |token| async move {
                    token.cancelled().await;
                    cancelled.notify_one();
                    release.notified().await;
                }
            })
            .unwrap();
        let mut builder = ContainerBuilder::new();
        builder.context(
            ConfigRepository::new(figment::Figment::new()),
            app_owner.registrar(),
        );
        builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
        let container = builder.build();
        let first = container.scope().unwrap();
        let second = container.scope().unwrap();
        first.resolve::<usize>().await.unwrap();
        second.resolve::<usize>().await.unwrap();
        let owner = Arc::clone(&app_owner);
        let shutdown = tokio::spawn(async move {
            owner
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
                .await
        });
        cancelled.notified().await;
        assert_eq!(
            first.resolve::<usize>().await.unwrap_err().code(),
            "scope_closed"
        );
        assert_eq!(
            second.resolve::<usize>().await.unwrap_err().code(),
            "scope_closed"
        );
        release.notify_one();
        shutdown.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn queued_scope_close_cancels_its_task_before_earlier_cleanup_finishes() {
        let app_owner = Arc::new(ResourceOwner::new());
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut builder = ContainerBuilder::new();
        builder.context(
            ConfigRepository::new(figment::Figment::new()),
            app_owner.registrar(),
        );
        builder.scoped::<usize, _>({
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            move |resolver| {
                let entered = Arc::clone(&entered);
                let release = Arc::clone(&release);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            entered.notify_one();
                            release.notified().await;
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(1))
                })
            }
        });
        let container = builder.build();
        let first = container.scope().unwrap();
        first.resolve::<usize>().await.unwrap();
        first.close();
        entered.notified().await;

        let second = Arc::new(container.scope().unwrap());
        let started = Arc::new(Notify::new());
        let cancelled = Arc::new(Notify::new());
        second
            .spawn(crate::core::ExecutionMetadata::default(), None, {
                let started = Arc::clone(&started);
                let cancelled = Arc::clone(&cancelled);
                move |token| async move {
                    started.notify_one();
                    token.cancelled().await;
                    cancelled.notify_one();
                }
            })
            .unwrap();
        started.notified().await;
        second.close();
        tokio::time::timeout(Duration::from_secs(1), cancelled.notified())
            .await
            .unwrap();
        release.notify_one();
        app_owner
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn manual_scope_close_runs_its_callback_without_app_owner() {
        let done = Arc::new(Notify::new());
        let mut builder = ContainerBuilder::new();
        builder.scoped::<usize, _>({
            let done = Arc::clone(&done);
            move |resolver| {
                let done = Arc::clone(&done);
                Box::pin(async move {
                    resolver.resources()?.on_shutdown(move || {
                        Box::pin(async move {
                            done.notify_one();
                            Ok(())
                        })
                    })?;
                    Ok(Arc::new(1))
                })
            }
        });
        let container = builder.build();
        let scope = container.scope().unwrap();
        scope.resolve::<usize>().await.unwrap();
        scope.close();
        tokio::time::timeout(Duration::from_secs(1), done.notified())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn singleton_saved_resolver_does_not_form_container_ownership_cycle() {
        struct Lazy(Resolver);
        let mut builder = ContainerBuilder::new();
        builder.singleton::<Lazy, _>(|r| Box::pin(async move { Ok(Arc::new(Lazy(r))) }));
        let container = builder.build();
        let weak = Arc::downgrade(&container.inner);
        let service = container.resolve::<Lazy>().await.unwrap();
        let saved = service.0.clone();
        drop(service);
        drop(container);
        assert!(weak.upgrade().is_none());
        assert_eq!(
            saved.resolve::<Lazy>().await.err().unwrap().code(),
            "container_closed"
        );
    }

    #[tokio::test]
    async fn published_singleton_can_use_saved_resolver_inside_waiter_notification() {
        struct Lazy(Resolver);
        struct Observer {
            inner: Weak<Inner>,
            outcome: AtomicUsize,
        }
        impl std::task::Wake for Observer {
            fn wake(self: Arc<Self>) {
                let resolver = {
                    let inner = self.inner.upgrade().unwrap();
                    let state = lock(&inner.state);
                    let attempt = state
                        .slots
                        .get(&CacheKey {
                            service: TypeId::of::<Lazy>(),
                            scope: u64::MAX,
                        })
                        .unwrap();
                    let outcome = lock(&attempt.outcome);
                    if let Some(Ok(value)) = outcome.as_ref() {
                        value.downcast_ref::<Arc<Lazy>>().unwrap().0.clone()
                    } else {
                        self.outcome.store(3, Ordering::Release);
                        return;
                    }
                };
                let mut future = resolver.resolve::<Lazy>();
                let mut context = std::task::Context::from_waker(std::task::Waker::noop());
                let result = match std::future::Future::poll(future.as_mut(), &mut context) {
                    std::task::Poll::Ready(Ok(_)) => 1,
                    std::task::Poll::Ready(Err(error)) if error.code() == "service_cycle" => 2,
                    _ => 3,
                };
                self.outcome.store(result, Ordering::Release);
            }
        }
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let mut builder = ContainerBuilder::new();
        let notice = Arc::clone(&started);
        let finish = Arc::clone(&release);
        builder.singleton::<Lazy, _>(move |r| {
            let notice = Arc::clone(&notice);
            let finish = Arc::clone(&finish);
            Box::pin(async move {
                notice.notify_one();
                finish.notified().await;
                Ok(Arc::new(Lazy(r)))
            })
        });
        let container = builder.build();
        let root = container.clone();
        let owner = tokio::spawn(async move { root.resolve::<Lazy>().await });
        started.notified().await;
        let attempt = Arc::clone(
            lock(&container.inner.state)
                .slots
                .get(&CacheKey {
                    service: TypeId::of::<Lazy>(),
                    scope: u64::MAX,
                })
                .unwrap(),
        );
        let observer = Arc::new(Observer {
            inner: Arc::downgrade(&container.inner),
            outcome: AtomicUsize::new(0),
        });
        let waker = std::task::Waker::from(Arc::clone(&observer));
        let mut context = std::task::Context::from_waker(&waker);
        let mut notified = Box::pin(attempt.changed.notified());
        assert!(std::future::Future::poll(notified.as_mut(), &mut context).is_pending());
        release.notify_one();
        owner.await.unwrap().unwrap();
        assert_eq!(
            observer.outcome.load(Ordering::Acquire),
            1,
            "已发布服务不能被仍活跃的历史帧误判循环"
        );
    }
    #[tokio::test]
    async fn cancellation_ends_factory_before_notifying_waiters() {
        struct Observer {
            activity: Arc<AtomicBool>,
            outcome: AtomicUsize,
        }
        impl std::task::Wake for Observer {
            fn wake(self: Arc<Self>) {
                self.outcome.store(
                    if self.activity.load(Ordering::Acquire) {
                        2
                    } else {
                        1
                    },
                    Ordering::Release,
                );
            }
        }
        let started = Arc::new(Notify::new());
        let saved = Arc::new(Mutex::new(None::<Resolver>));
        let mut builder = ContainerBuilder::new();
        let notice = Arc::clone(&started);
        let slot = Arc::clone(&saved);
        builder.singleton::<usize, _>(move |resolver| {
            *lock(&slot) = Some(resolver);
            notice.notify_one();
            Box::pin(std::future::pending())
        });
        let container = builder.build();
        let root = container.clone();
        let owner = tokio::spawn(async move { root.resolve::<usize>().await });
        started.notified().await;
        let attempt = Arc::clone(lock(&container.inner.state).slots.values().next().unwrap());
        let activity = Arc::clone(&lock(&saved).as_ref().unwrap().chain.last().unwrap().2);
        let observer = Arc::new(Observer {
            activity,
            outcome: AtomicUsize::new(0),
        });
        let waker = std::task::Waker::from(Arc::clone(&observer));
        let mut context = std::task::Context::from_waker(&waker);
        let mut notified = Box::pin(attempt.changed.notified());
        assert!(std::future::Future::poll(notified.as_mut(), &mut context).is_pending());
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());
        assert_eq!(observer.outcome.load(Ordering::Acquire), 1);
        assert_eq!(
            attempt.wait().await.unwrap_err().code(),
            "service_initialization_cancelled"
        );
    }

    #[tokio::test]
    async fn completed_factory_removes_detached_child_wait_edges() {
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let task = Arc::new(Mutex::new(None));
        let mut builder = ContainerBuilder::new();
        let notice = Arc::clone(&started);
        let finish = Arc::clone(&release);
        builder.singleton::<u8, _>(move |_| {
            let notice = Arc::clone(&notice);
            let finish = Arc::clone(&finish);
            Box::pin(async move {
                notice.notify_one();
                finish.notified().await;
                Ok(Arc::new(7))
            })
        });
        let child_task = Arc::clone(&task);
        builder.singleton::<usize, _>(move |resolver| {
            let started = Arc::clone(&started);
            let task = Arc::clone(&child_task);
            Box::pin(async move {
                *lock(&task) = Some(tokio::spawn(async move { resolver.resolve::<u8>().await }));
                started.notified().await;
                Ok(Arc::new(1))
            })
        });
        let container = builder.build();
        assert_eq!(*container.resolve::<usize>().await.unwrap(), 1);
        assert!(lock(&container.inner.state).edges.is_empty());
        release.notify_one();
        let task = lock(&task).take().unwrap();
        assert_eq!(*task.await.unwrap().unwrap(), 7);
        assert!(lock(&container.inner.state).edges.is_empty());
    }
    #[tokio::test]
    async fn scope_close_and_root_reset_notify_after_releasing_internal_locks() {
        struct Observer {
            inner: Weak<Inner>,
            attempt: Weak<Attempt>,
            calls: AtomicUsize,
        }
        impl std::task::Wake for Observer {
            fn wake(self: Arc<Self>) {
                let inner = self.inner.upgrade().unwrap();
                let attempt = self.attempt.upgrade().unwrap();
                assert!(inner.state.try_lock().is_ok(), "通知不能持有 State 锁");
                assert!(attempt.outcome.try_lock().is_ok(), "通知不能持有结果锁");
                let _scope = Container { inner }.scope().unwrap();
                self.calls.fetch_add(1, Ordering::AcqRel);
            }
        }
        for root_reset in [false, true] {
            let mut builder = ContainerBuilder::new();
            builder.scoped::<usize, _>(|_| Box::pin(std::future::pending()));
            let container = builder.build();
            let scope = container.scope().unwrap();
            let resolver = if root_reset {
                let state = lock(&container.inner.state);
                container.resolver(
                    state.root_scope,
                    Arc::clone(&state.root_closed),
                    state.root_resources.registrar(),
                )
            } else {
                container.resolver(
                    scope.id,
                    Arc::clone(&scope.closed),
                    scope.resources.registrar(),
                )
            };
            let mut future = resolver.resolve::<usize>();
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            assert!(std::future::Future::poll(future.as_mut(), &mut context).is_pending());
            let attempt = Arc::clone(lock(&container.inner.state).slots.values().next().unwrap());
            let observer = Arc::new(Observer {
                inner: Arc::downgrade(&container.inner),
                attempt: Arc::downgrade(&attempt),
                calls: AtomicUsize::new(0),
            });
            let waker = std::task::Waker::from(Arc::clone(&observer));
            let mut context = std::task::Context::from_waker(&waker);
            let mut closed = Box::pin(resolver.closed.changed.notified());
            let mut published = Box::pin(attempt.changed.notified());
            assert!(std::future::Future::poll(closed.as_mut(), &mut context).is_pending());
            assert!(std::future::Future::poll(published.as_mut(), &mut context).is_pending());
            if root_reset {
                container
                    .forget_scoped_instances(tokio::time::Instant::now() + Duration::from_secs(1))
                    .await
                    .unwrap();
            } else {
                scope.close();
            }
            assert_eq!(observer.calls.load(Ordering::Acquire), 2);
            assert_eq!(future.await.unwrap_err().code(), "scope_closed");
        }
    }
}

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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use tokio::sync::Notify;

use super::{BoxFuture, Error, Result};

type Value = Arc<dyn Any + Send + Sync>;
type Factory = Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

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

struct Binding {
    lifetime: Lifetime,
    factory: Factory,
}

/// 注册服务定义；消费 builder 后得到不再重绑的容器。
#[derive(Default)]
pub struct ContainerBuilder {
    bindings: HashMap<TypeId, Binding>,
}

impl ContainerBuilder {
    /// 创建空定义集合。
    pub fn new() -> Self {
        Self::default()
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
        self.singleton::<T, _>(move |_| {
            let value = Arc::clone(&value);
            Box::pin(async move { Ok(value) })
        })
    }

    fn register<T, F>(&mut self, lifetime: Lifetime, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        let factory = Arc::new(factory);
        self.bindings.insert(
            TypeId::of::<T>(),
            Binding {
                lifetime,
                factory: Arc::new(move |resolver| {
                    let factory = Arc::clone(&factory);
                    Box::pin(async move { Ok(Arc::new(factory(resolver).await?) as Value) })
                }),
            },
        );
        self
    }

    /// 封闭定义集合，创建根生命周期；没有进程全局容器。
    pub fn build(self) -> Container {
        Container {
            inner: Arc::new(Inner {
                bindings: self.bindings,
                state: Mutex::new(State::default()),
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
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.changed.notify_waiters();
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
    next_id: u64,
    root_scope: u64,
    slots: HashMap<CacheKey, Arc<Attempt>>,
    edges: HashMap<(u64, u64), usize>,
    root_closed: Arc<ScopeStatus>,
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
    bindings: HashMap<TypeId, Binding>,
    state: Mutex<State>,
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
    finished: bool,
}

impl Construction {
    fn finish(&mut self, result: std::result::Result<Value, Failure>) {
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

impl Container {
    /// 在根生命周期解析；`scoped` 在根也会共享，符合 Laravel 行为。
    pub async fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        let (scope, closed) = {
            let state = lock(&self.inner.state);
            (state.root_scope, Arc::clone(&state.root_closed))
        };
        self.resolver(scope, closed).resolve().await
    }

    /// 清除根 lifecycle 的 scoped 实例，下一次解析重新构造。
    ///
    /// 当前基元没有异步清理回调；资源回收协议须在 Provider 集成前补齐。
    pub fn forget_scoped_instances(&self) -> Result<()> {
        let mut state = lock(&self.inner.state);
        let next = state.id()?;
        let old = state.root_scope;
        state.root_scope = next;
        state.root_closed.close();
        state.root_closed = Arc::new(ScopeStatus::default());
        let removed = close_scope(&mut state, old);
        drop(state);
        // 用户服务的 Drop 可能再次访问容器，不能在持有内部锁时销毁它。
        drop(removed);
        Ok(())
    }

    /// 创建独立生命周期，不继承根或其他 scope 的 scoped 实例。
    pub fn scope(&self) -> Result<Scope> {
        let id = lock(&self.inner.state).id()?;
        Ok(Scope {
            container: self.clone(),
            id,
            closed: Arc::new(ScopeStatus::default()),
        })
    }

    fn resolver(&self, scope: u64, closed: Arc<ScopeStatus>) -> Resolver {
        Resolver {
            container: Arc::downgrade(&self.inner),
            scope,
            chain: Vec::new(),
            constructing: None,
            captive: false,
            closed,
        }
    }
}

/// 手动生命周期基元；请求/队列入口将在统一 Provider 集成阶段接入。
pub struct Scope {
    container: Container,
    id: u64,
    closed: Arc<ScopeStatus>,
}

impl Scope {
    /// 在本生命周期解析服务。
    pub async fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.container
            .resolver(self.id, Arc::clone(&self.closed))
            .resolve()
            .await
    }

    /// 关闭本生命周期，后续解析失败；本基元尚不登记异步资源回收。
    pub fn close(&self) {
        let mut state = lock(&self.container.inner.state);
        self.closed.close();
        let removed = close_scope(&mut state, self.id);
        drop(state);
        drop(removed);
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.close();
    }
}

fn close_scope(state: &mut State, scope: u64) -> Vec<Arc<Attempt>> {
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
            attempt.changed.notify_waiters();
        }
    }
    removed
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
}

impl Resolver {
    fn container(&self) -> Result<Container> {
        self.container
            .upgrade()
            .map(|inner| Container { inner })
            .ok_or_else(|| container_error("container_closed", "容器已释放"))
    }

    /// 解析具体类型或 `dyn Trait`；缺失、环和构造失败返回错误。
    pub fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> BoxFuture<'_, Result<Arc<T>>> {
        Box::pin(async move {
            let container = self.container()?;
            if self.closed.closed.load(Ordering::Acquire) {
                return Err(container_error("scope_closed", "生命周期已关闭"));
            }
            let key = TypeId::of::<T>();
            let name = type_name::<T>();
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
            let binding = container.inner.bindings.get(&key).ok_or_else(|| {
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
            })?;
            if binding.lifetime == Lifetime::Scoped && self.captive {
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
            child.chain.push((key, name, activity));
            if binding.lifetime == Lifetime::Singleton {
                child.captive = true;
                child.scope = u64::MAX;
                child.closed = Arc::new(ScopeStatus::default());
            }
            let value = if binding.lifetime == Lifetime::Bind {
                tokio::select! { result = (binding.factory)(child) => result?, () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")) }
            } else {
                self.cached(binding, child, key, name).await?
            };
            if self.closed.closed.load(Ordering::Acquire) {
                return Err(container_error("scope_closed", "生命周期已关闭"));
            }
            value.downcast_ref::<Arc<T>>().cloned().ok_or_else(|| {
                container_error(
                    "service_type_mismatch",
                    format!("服务 `{name}` 内部类型不匹配"),
                )
            })
        })
    }

    async fn cached(
        &self,
        binding: &Binding,
        mut child: Self,
        service: TypeId,
        name: &'static str,
    ) -> Result<Value> {
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
        let (attempt, owner, _edge) = {
            let mut state = lock(&container.inner.state);
            if self.closed.closed.load(Ordering::Acquire) {
                return Err(container_error("scope_closed", "生命周期已关闭"));
            }
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
            let edge = if let Some((parent, _)) = self
                .constructing
                .as_ref()
                .filter(|(_, active)| active.load(Ordering::Acquire))
            {
                let parent = *parent;
                if let Some(mut path) = state.path(attempt.id, parent) {
                    path.insert(0, parent);
                    let names = path
                        .iter()
                        .filter_map(|id| {
                            state
                                .slots
                                .values()
                                .find(|slot| slot.id == *id)
                                .map(|slot| slot.name)
                        })
                        .collect::<Vec<_>>();
                    return Err(container_error("service_cycle", names.join(" → ")));
                }
                *state.edges.entry((parent, attempt.id)).or_default() += 1;
                Some(WaitEdge {
                    inner: Arc::clone(&container.inner),
                    edge: (parent, attempt.id),
                })
            } else {
                None
            };
            (attempt, owner, edge)
        };
        if !owner {
            return tokio::select! {
                result = attempt.wait() => result,
                () = self.closed.wait_closed() => Err(container_error("scope_closed", "生命周期已关闭")),
            };
        }
        let mut guard = Construction {
            inner: Arc::clone(&container.inner),
            key,
            attempt: Arc::clone(&attempt),
            finished: false,
        };
        child.constructing = child
            .chain
            .last()
            .map(|(_, _, active)| (attempt.id, Arc::clone(active)));
        let outcome = tokio::select! {
            result = (binding.factory)(child) => result,
            closed = attempt.wait() => return closed,
            () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
        };
        match outcome {
            Ok(value) => {
                guard.finish(Ok(Arc::clone(&value)));
                attempt.wait().await
            }
            Err(error) => {
                let failure = Failure::initialization(name, error);
                guard.finish(Err(failure));
                attempt.wait().await
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
    struct Hello;
    impl Greeting for Hello {
        fn greet(&self) -> &'static str {
            "你好"
        }
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
        container.forget_scoped_instances().unwrap();
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
        container.forget_scoped_instances().unwrap();
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
        container.forget_scoped_instances().unwrap();
        assert!(dropped.load(Ordering::Acquire));
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
}

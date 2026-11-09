//! 类型化服务容器基元；生产应用接入由统一 Provider 迁移完成。
//!
//! ```
//! use openrust::container::ContainerBuilder;
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

#[cfg(test)]
use std::any::Any;
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use tokio::sync::Notify;

tokio::task_local! {
    static ENTRY_SCOPE: Arc<Scope>;
}

use crate::config::{ConfigError, Environment, ServerConfig, repository::ConfigRepository};
use crate::foundation::resources::ResourceOwner;
use crate::foundation::resources::ResourceRegistrar;

use crate::{BoxFuture, Error, Result};

type Value = Arc<resolved_value::ResolvedValue>;
type Factory = Arc<dyn Fn(Resolver) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

mod service_key;
pub use service_key::ServiceKey;

mod class_binding;
mod class_identity;
use class_descriptor::ClassRegistry;
pub use class_descriptor::{
    ClassDescriptor, ConstructorParam, PhpBytes, PhpIntBool, PhpStringable, PhpTarget,
    ResolvedArgs, ScalarDiagnostic,
};
pub(crate) mod scalar_diagnostics;
pub use class_projection::ClassProjection;
pub use scalar_diagnostics::ScalarDiagnostics;

#[cfg(test)]
#[path = "container/descriptor_race_tests.rs"]
mod descriptor_race_tests;
mod descriptor_resolve;
pub use descriptor_resolve::{CallArguments, MethodCall, StaticCall};
use descriptor_resolve::{MethodConsumer, MethodRegistry};
#[cfg(test)]
#[path = "container/descriptor_resolve_tests.rs"]
mod descriptor_resolve_tests;
mod descriptor_source;
use descriptor_source::BindingSource;
mod directory;
mod resolved_value;
use resolved_value::{erase, plain};
mod resolving_hooks;
pub use directory::{BindingSnapshot, FactoryHandle};
pub use resolving_hooks::ResolutionEvent;
use resolving_hooks::ResolveHooks;
#[cfg(test)]
#[path = "container/attribute_resolution_tests.rs"]
mod attribute_resolution_tests;
pub mod attributes;
use attributes::AttributeRegistry;
pub use attributes::{
    AttributeArguments, AttributeEvent, AttributeParameter, AttributeValue, ClassAttribute,
    ContextualAttribute,
};
mod self_building;
pub use self_building::SelfBuilding;
use self_building::SelfBuildingBindings;
mod contextual;
use contextual::ContextualBindings;
pub use contextual::{
    ContextualBindingBuilder, ContextualParameterBinding, ContextualServiceBinding,
};
mod parameters;
use parameters::{ParameterFrame, ParameterView};
pub use parameters::{ParameterKey, ParameterOverrides, ParameterValue};

mod extensions;
use extensions::{CachedAdmission, Extender, ExtensionState, Rebound};

fn container_error(code: &str, message: impl Into<String>) -> Error {
    Error::custom_with_status(http::StatusCode::INTERNAL_SERVER_ERROR, code, message)
}

#[derive(Debug, thiserror::Error)]
#[error("容器依赖缺失或无法构造")]
struct UnresolvableDependency;

fn unresolvable_dependency(code: &str, message: impl Into<String>) -> Error {
    Error::http_with_source(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        code,
        message,
        UnresolvableDependency,
    )
}

fn is_unresolvable_dependency(error: &Error) -> bool {
    match error {
        Error::HttpWithSource { source, .. } => source.is::<UnresolvableDependency>(),
        Error::ServiceInitialization { source, .. } => is_unresolvable_dependency(source),
        _ => false,
    }
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// raw入参仅由本持有者交给Resolver；已知独立entries在任何退役路径均逐项释放。
struct FactoryParameters {
    overrides: ParameterOverrides,
    owner: Option<ResourceRegistrar>,
    completion: Option<crate::foundation::resources::ResourceTaskCompletion>,
}
impl FactoryParameters {
    fn empty() -> Self {
        Self {
            overrides: ParameterOverrides::new(),
            owner: None,
            completion: None,
        }
    }
}
impl Drop for FactoryParameters {
    fn drop(&mut self) {
        use crate::foundation::lifecycle::disposal::{self, Credential};
        let mut cleanup = if self.completion.is_none() {
            self.owner
                .as_ref()
                .and_then(ResourceRegistrar::begin_cleanup)
        } else {
            None
        };
        let original = self
            .completion
            .as_ref()
            .map(|completion| Credential::Completion(completion.clone()))
            .or_else(|| {
                cleanup
                    .as_ref()
                    .map(|cleanup| Credential::Cleanup(cleanup.disposal_ticket()))
            });
        let ((), report) = disposal::scope(original.clone(), || {
            let failures = crate::foundation::lifecycle::drop_all_isolated(std::mem::take(
                &mut self.overrides.entries,
            ));
            if failures != 0 {
                disposal::failure(original.as_ref());
            }
        });
        if report.seen
            && let Some(completion) = &self.completion
        {
            completion.note_diagnostic();
        }
        // 本次TLS已恢复，才完成真实原清理票据；没有共享终态供别的Arc观察者归责。
        if let Some(cleanup) = &mut cleanup {
            cleanup.finish(0);
        }
    }
}

struct PreviousFactoryInputs {
    parameters: Arc<FactoryParameters>,
    completion: Option<crate::foundation::resources::ResourceTaskCompletion>,
}
impl PreviousFactoryInputs {
    fn dispose(
        self,
        credential: Option<crate::foundation::lifecycle::disposal::Credential>,
    ) -> bool {
        use crate::foundation::lifecycle::disposal;
        let ((), report) = disposal::scope(credential, || {
            disposal::dispose(self.parameters, None);
            #[cfg(test)]
            directory_tests_v10::after_previous_release();
            disposal::dispose(self.completion, None);
        });
        report.seen
    }
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
    pending: Vec<Extender>,
    lifetime: Lifetime,
    factory: Option<Factory>,
    factory_output: Option<TypeId>,
    class_factory: Option<Arc<class_binding::ClassFactory>>,
    instance: Option<Value>,
}

impl Binding {
    fn for_resolution(&self, with_parameters: bool) -> Option<Self> {
        if with_parameters && self.factory.is_none() {
            return None;
        }
        let mut binding = self.clone();
        if with_parameters {
            binding.instance = None;
            binding.pending.clear();
            binding.lifetime = Lifetime::Bind;
        }
        Some(binding)
    }

    fn output_projection(
        &self,
        classes: &ClassRegistry,
        request: &ResolveRequest,
    ) -> Result<Option<ClassProjection>> {
        let (Some(expected), Some(target)) = (request.expected_output(), request.target()) else {
            return Ok(None);
        };
        let declared = self
            .instance
            .as_ref()
            .map(|value| value.as_ref().type_id())
            .or(self.factory_output);
        if declared == Some(expected) {
            return Ok(None);
        }
        declared
            .and_then(|declared| classes.output_projection(declared, target).cloned())
            .map(Some)
            .ok_or_else(|| container_error("service_type_mismatch", "服务声明输出与请求类型不同"))
    }

    fn effective_lifetime(&self) -> Lifetime {
        if self.instance.is_some() {
            Lifetime::Singleton
        } else {
            self.lifetime
        }
    }

    fn instance_only(value: Value) -> Self {
        Self {
            generation: 0,
            pending: Vec::new(),
            lifetime: Lifetime::Singleton,
            factory: None,
            factory_output: None,
            class_factory: None,
            instance: Some(value),
        }
    }
}

enum BindingChange {
    Factory(Binding),
    Instance(Value),
    ForgetInstance,
}

impl BindingChange {
    // 无失败出口；调用者先完成所有容量/关闭检查，再在同锁内选择新条目。
    // 克隆覆盖里的值仍由原条目持有，最后的用户Drop交给锁外的previous。
    fn replacement(self, existing: Option<&Binding>) -> Option<Binding> {
        match self {
            BindingChange::Factory(binding) => Some(binding),
            BindingChange::Instance(value) => {
                let mut binding = existing
                    .cloned()
                    .unwrap_or_else(|| Binding::instance_only(Arc::clone(&value)));
                binding.pending.clear();
                binding.instance = Some(value);
                Some(binding)
            }
            BindingChange::ForgetInstance => existing
                .filter(|binding| binding.factory.is_some())
                .cloned()
                .map(|mut binding| {
                    binding.instance = None;
                    binding.pending.clear();
                    binding
                }),
        }
    }
}

/// 注册启动阶段服务定义；消费 builder 后得到可动态重绑的同一容器。
#[derive(Default)]
pub struct ContainerBuilder {
    bindings: HashMap<ServiceKey, Binding>,
    binding_order: Vec<ServiceKey>,
    aliases: directory::AliasState,
    contextual: ContextualBindings,
    hooks: ResolveHooks,
    attributes: AttributeRegistry,
    self_building: SelfBuildingBindings,
    methods: MethodRegistry,
    classes: Arc<ClassRegistry>,
    extensions: ExtensionState,
    config: Option<ConfigRepository>,
    resources: Option<ResourceRegistrar>,
    scalar_diagnostics: ScalarDiagnostics,
}

impl ContainerBuilder {
    /// 创建空定义集合。
    pub fn new() -> Self {
        Self::with_class_registry(ClassRegistry::default())
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
        self.instance_key(ServiceKey::typed::<T>(), value);
        self
    }

    /// 同步读取已登记实例；异步 factory 必须通过 `resolve` 构造。
    pub fn get_instance<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.bindings
            .get(&ServiceKey::typed::<T>())
            .filter(|binding| binding.pending.is_empty())
            .and_then(|binding| binding.instance.as_ref())
            .and_then(|value| value.downcast_ref::<Arc<T>>())
            .cloned()
    }

    /// 检查类型是否已绑定，无论其构造方式。
    pub fn contains<T: ?Sized + Send + Sync + 'static>(&self) -> bool {
        self.bound_key(&ServiceKey::typed::<T>())
    }

    fn register<T, F>(&mut self, lifetime: Lifetime, factory: F) -> &mut Self
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(ServiceKey::typed::<T>(), lifetime, factory, false);
        self
    }

    /// 为显式弱标量构造参数安装同步诊断处理。处理失败会阻止构造。
    pub fn scalar_diagnostics(&mut self, policy: ScalarDiagnostics) -> &mut Self {
        self.scalar_diagnostics = policy;
        self
    }

    /// 封闭定义集合，创建根生命周期；没有进程全局容器。
    pub fn build(self) -> Container {
        // 未装配 Application 的容器也有应用级单例，根 scoped 重置不能回收它们。
        let standalone_resources = self.resources.is_none().then(ResourceOwner::new);
        let resources = self
            .resources
            .or_else(|| standalone_resources.as_ref().map(ResourceOwner::registrar));
        Container {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    bindings: self.bindings,
                    binding_order: self.binding_order,
                    aliases: self.aliases,
                    contextual: self.contextual,
                    hooks: self.hooks,
                    attributes: self.attributes,
                    self_building: self.self_building,
                    methods: self.methods,
                    classes: self.classes,
                    extensions: self.extensions,
                    ..State::default()
                }),
                reset: tokio::sync::Mutex::new(()),
                config: self.config,
                resources,
                standalone_resources,
                scalar_diagnostics: self.scalar_diagnostics,
            }),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    service: ServiceKey,
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

/// 首次根封闭的清理证据，复用现有解析记录的完成票据。
#[derive(Default)]
struct RootRawClose {
    deadline: Option<tokio::time::Instant>,
    pending: Vec<crate::foundation::resources::ResourceTaskCompletion>,
    interrupted: bool,
}

#[derive(Default)]
struct State {
    bindings: HashMap<ServiceKey, Binding>,
    binding_environment: Option<Environment>,
    checked_binding_declarations: HashSet<ServiceKey>,
    binding_discoveries: HashMap<ServiceKey, Arc<class_binding::InFlightBinding>>,
    binding_changes: Arc<Notify>,
    binding_order: Vec<ServiceKey>,
    resolved_history: HashSet<ServiceKey>,
    aliases: directory::AliasState,
    contextual: ContextualBindings,
    hooks: ResolveHooks,
    attributes: AttributeRegistry,
    self_building: SelfBuildingBindings,
    methods: MethodRegistry,
    classes: Arc<ClassRegistry>,
    raw_attempts: HashMap<u64, directory::RawRecord>,
    root_raw_close: Option<RootRawClose>,
    termination: Arc<ScopeStatus>,
    next_generation: u64,
    flush_revision: u64,
    // 只记录本轮目录中实际变更过的类；其余类沿用最近一次 flush 的版本。
    class_source_revisions: HashMap<ServiceKey, u64>,
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
    fn resolution_closure_error(&self) -> Error {
        if self.terminated {
            container_error("container_closed", "容器已关闭")
        } else {
            container_error("scope_closed", "生命周期已关闭")
        }
    }

    fn next_source_revision(&self) -> Result<u64> {
        self.next_generation
            .checked_add(1)
            .ok_or_else(|| container_error("container_capacity_exceeded", "服务绑定版本已耗尽"))
    }

    fn publish_source_revision(&mut self, service: &ServiceKey, revision: u64) {
        self.next_generation = revision;
        if self.classes.class(service).is_some() {
            self.class_source_revisions
                .insert(service.clone(), revision);
        }
    }

    fn has_published_instance(&self, service: &ServiceKey) -> bool {
        self.bindings
            .get(service)
            .is_some_and(|binding| binding.instance.is_some())
            || self.slots.iter().any(|(key, attempt)| {
                &key.service == service
                    && lock(&attempt.outcome)
                        .as_ref()
                        .is_some_and(std::result::Result::is_ok)
            })
    }

    fn record_success(&mut self, key: &CacheKey) {
        self.extensions.resolved.insert(key.clone());
        let binding = self.bindings.get(&key.service);
        if binding.map_or_else(
            || self.service_lifetime(&key.service, None) != Lifetime::Bind,
            |binding| binding.instance.is_none() && binding.factory.is_some(),
        ) {
            self.resolved_history.insert(key.service.clone());
        }
    }

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
    standalone_resources: Option<ResourceOwner>,
    scalar_diagnostics: ScalarDiagnostics,
}

#[derive(Clone)]
struct Failure {
    code: &'static str,
    message: String,
    source: Option<(ServiceKey, Arc<Error>)>,
    unresolvable: bool,
}

impl Failure {
    fn initialization(service: &ServiceKey, error: Error) -> Self {
        match error.code() {
            "service_not_bound" => Failure {
                code: "service_not_bound",
                message: error.to_string(),
                source: None,
                unresolvable: is_unresolvable_dependency(&error),
            },
            "service_cycle" => Failure {
                code: "service_cycle",
                message: error.to_string(),
                source: None,
                unresolvable: false,
            },
            "captive_scope_dependency" => Failure {
                code: "captive_scope_dependency",
                message: error.to_string(),
                source: None,
                unresolvable: false,
            },
            "scope_closed" => Failure {
                code: "scope_closed",
                message: error.to_string(),
                source: None,
                unresolvable: false,
            },
            _ => Failure {
                code: "service_initialization_failed",
                message: String::new(),
                source: Some((service.clone(), Arc::new(error))),
                unresolvable: false,
            },
        }
    }

    fn error(&self) -> Error {
        if let Some((service, source)) = &self.source {
            Error::ServiceInitialization {
                service: crate::core::error::ServiceIdentity::from_name(service.identity()),
                source: Arc::clone(source),
            }
        } else if self.unresolvable {
            unresolvable_dependency(self.code, self.message.clone())
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
    service: ServiceKey,
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
        self.publish(result, true);
    }

    fn publish(&mut self, result: std::result::Result<Value, Failure>, record_history: bool) {
        // 发布结果前结束构造帧；通知可以同步唤醒并重新进入保存的 Resolver。
        self.activity.store(false, Ordering::Release);
        let mut unclaimed = Some(result);
        {
            let mut state = lock(&self.inner.state);
            let mut outcome = lock(&self.attempt.outcome);
            state.edges.retain(|(from, _), _| *from != self.attempt.id);
            let current = state
                .slots
                .get(&self.key)
                .is_some_and(|slot| slot.id == self.attempt.id);
            #[cfg(test)]
            binding_event_tests::observe_publication();
            if outcome.is_none() {
                *outcome = unclaimed.take();
                if current {
                    if outcome.as_ref().is_some_and(std::result::Result::is_ok) && !state.terminated
                    {
                        if record_history {
                            state.record_success(&self.key);
                        }
                    } else {
                        state.slots.remove(&self.key);
                    }
                }
            }
        }
        // 关闭可能已决定 outcome；输掉发布的值及任意用户析构都留在锁外。
        drop(unclaimed);
        self.finished = true;
        self.attempt.changed.notify_waiters();
    }
    fn record_success_if_current(&self, resolver: &Resolver) {
        let mut state = lock(&self.inner.state);
        if resolver.check_open(&state).is_ok()
            && state
                .slots
                .get(&self.key)
                .is_some_and(|slot| slot.id == self.attempt.id)
        {
            state.record_success(&self.key);
        }
    }
}

impl Drop for Construction {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(Err(Failure {
                code: "service_initialization_cancelled",
                message: "服务构造已取消；后续独立解析可重试".into(),
                source: None,
                unresolvable: false,
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

/// 已离开容器目录的对象。应用先撤销自己的注册，再释放这些对象。
#[must_use]
pub(crate) struct FlushedContainer {
    bindings: HashMap<ServiceKey, Binding>,
    slots: HashMap<CacheKey, Arc<Attempt>>,
    discoveries: HashMap<ServiceKey, Arc<class_binding::InFlightBinding>>,
    operations: HashMap<CacheKey, Arc<extensions::Operation>>,
    changes: Arc<Notify>,
    hooks: ResolveHooks,
    listeners: HashMap<ServiceKey, Vec<extensions::Rebinding>>,
}

impl FlushedContainer {
    pub(crate) fn dispose(self) -> bool {
        use crate::foundation::lifecycle::disposal;
        self.changes.notify_waiters();
        for operation in self.operations.into_values() {
            operation.release();
        }
        drop(self.discoveries);
        let mut failed = self.hooks.dispose();
        for listener in self.listeners.into_values().flatten() {
            failed |= disposal::dispose(listener, None).seen;
        }
        for binding in self.bindings.into_values() {
            // 同一绑定的工厂、实例与待初始化装饰器可能分别拥有用户析构。
            failed |= disposal::dispose(binding.factory, None).seen;
            failed |= disposal::dispose(binding.instance, None).seen;
            for extender in binding.pending {
                failed |= disposal::dispose(extender, None).seen;
            }
        }
        for slot in self.slots.into_values() {
            failed |= disposal::dispose(slot, None).seen;
        }
        failed
    }
}

/// 弱引用同一容器的入口 scope 创建句柄；长寿命 singleton 持有它不会形成引用环。
#[derive(Clone)]
pub struct ScopeFactory {
    inner: Weak<Inner>,
}

impl ScopeFactory {
    pub(crate) fn container(&self) -> Option<Container> {
        self.inner.upgrade().map(|inner| Container { inner })
    }

    /// 为一次独立入口创建新 scope；容器已释放或关闭时返回错误。
    pub fn scope(&self) -> Result<Scope> {
        self.container()
            .ok_or_else(|| container_error("container_closed", "容器已关闭"))?
            .scope()
    }
}

impl Container {
    #[cfg(test)]
    async fn replace_binding(&self, service: ServiceKey, binding: Binding) -> Result<()> {
        self.apply_binding_change(service, BindingChange::Factory(binding))
            .await
    }

    async fn apply_binding_change(&self, service: ServiceKey, change: BindingChange) -> Result<()> {
        if let Some(event) = self.commit_binding_change(service, change)? {
            event.dispatch().await?;
        }
        Ok(())
    }

    fn commit_binding_change(
        &self,
        service: ServiceKey,
        change: BindingChange,
    ) -> Result<Option<Rebound>> {
        self.commit_binding_change_if(service, change, false)
            .map(|(_, event)| event)
    }

    fn commit_binding_change_if(
        &self,
        service: ServiceKey,
        change: BindingChange,
        only_if_unbound: bool,
    ) -> Result<(directory::RegistrationOutcome, Option<Rebound>)> {
        self.commit_binding_change_when(service, change, |state, service| {
            Ok(!only_if_unbound || !state.bound_key(service))
        })
    }

    fn commit_binding_change_when(
        &self,
        service: ServiceKey,
        change: BindingChange,
        admit: impl FnOnce(&State, &ServiceKey) -> Result<bool>,
    ) -> Result<(directory::RegistrationOutcome, Option<Rebound>)> {
        let (previous, removed, operations, event, binding_changes) = {
            let mut state = lock(&self.inner.state);
            if state.terminated {
                return Err(container_error("container_closed", "容器已关闭"));
            }
            let service = state.classes.canonical_key_owned(service);
            let was_bound = state.bound_key(&service);
            if !admit(&state, &service)? {
                return Ok((directory::RegistrationOutcome::AlreadyBound, None));
            }
            let forgetting = matches!(change, BindingChange::ForgetInstance);
            if forgetting && !state.has_published_instance(&service) {
                return Ok((directory::RegistrationOutcome::Registered, None));
            }
            let revision = state.next_source_revision()?;
            let installing_instance = matches!(change, BindingChange::Instance(_));
            let mut replacement = change.replacement(state.bindings.get(&service));
            if let Some(binding) = &mut replacement {
                binding.generation = revision;
            }
            let previously_resolved = state.resolved_history.contains(&service);
            let notify = !forgetting
                && (if installing_instance {
                    was_bound
                } else {
                    previously_resolved
                });
            let event = replacement.as_ref().filter(|_| notify).and_then(|binding| {
                let root = self.resolver(
                    state.root_scope,
                    Arc::clone(&state.root_closed),
                    state.root_resources.registrar(),
                );
                Rebound::capture(
                    &state,
                    &self.inner,
                    &root,
                    &service,
                    binding,
                    binding.instance.clone(),
                )
            });
            if !forgetting {
                state.aliases.remove_at(&service, revision);
            }
            state.publish_source_revision(&service, revision);
            state.extensions.discard_pending(&service);
            state
                .extensions
                .resolved
                .retain(|key| key.service != service);
            let operations = state.extensions.discard_operations(&service);
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
                .cloned()
                .collect();
            let removed: Vec<_> = keys
                .into_iter()
                .filter_map(|key| state.slots.remove(&key))
                .collect();
            if replacement
                .as_ref()
                .is_some_and(|binding| binding.factory.is_some())
                && state
                    .bindings
                    .get(&service)
                    .is_none_or(|binding| binding.factory.is_none())
            {
                state.binding_order.push(service.clone());
            }
            let previous = match replacement {
                Some(binding) => state.bindings.insert(service, binding),
                None => state.bindings.remove(&service),
            };
            (
                previous,
                removed,
                operations,
                event,
                Arc::clone(&state.binding_changes),
            )
        };
        binding_changes.notify_waiters();
        for operation in &operations {
            operation.release();
        }
        drop(previous);
        drop(removed);
        drop(operations);
        Ok((directory::RegistrationOutcome::Registered, event))
    }

    /// 安装用于声明式 `Bind` 的环境。已完成的声明查找保留原有结果。
    pub fn set_binding_environment(&self, environment: Environment) -> Result<()> {
        let mut state = lock(&self.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        if state.binding_environment.as_ref() != Some(&environment) {
            state.binding_environment = Some(environment);
            let changes = Arc::clone(&state.binding_changes);
            drop(state);
            changes.notify_waiters();
        }
        Ok(())
    }

    pub(crate) fn use_application_binding_environment(
        &self,
        config: &ConfigRepository,
    ) -> Result<()> {
        if lock(&self.inner.state).classes.has_environment_bindings() {
            let app = config.get("app")?;
            let environment = app
                .as_ref()
                .and_then(|app| app.get("env"))
                .and_then(serde_json::Value::as_str)
                .map(|name| Environment::from(name.to_owned()))
                .unwrap_or_default();
            self.set_binding_environment(environment)?;
        }
        Ok(())
    }

    async fn register<T, F>(&self, lifetime: Lifetime, factory: F) -> Result<()>
    where
        T: ?Sized + Send + Sync + 'static,
        F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
    {
        self.register_key::<T, F>(ServiceKey::typed::<T>(), lifetime, factory, false)
            .await
            .map(|_| ())
    }

    /// 动态注册每次解析均调用的工厂；覆盖后新解析使用新定义。
    ///
    /// 工厂重绑定通知按提交的代次准入；准入前被替换将返回 `binding_replaced`。
    /// 登记已经提交；通知错误不会回滚，已准入的旧构造只向该次事件交付结果。
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
        self.instance_key(ServiceKey::typed::<T>(), value).await
    }

    /// 忘记此原始类型键的实例与缓存，保留原工厂及成功构造历史。
    ///
    /// 不触发重绑定回调，不关闭仍由调用方持有的`Arc`或其资源owner。
    pub async fn forget_instance<T: ?Sized + Send + Sync + 'static>(&self) -> Result<()> {
        self.forget_instance_key(ServiceKey::typed::<T>()).await
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
        lock(&self.inner.state).bound_key(&ServiceKey::typed::<T>())
    }

    pub(crate) fn is_terminated(&self) -> bool {
        lock(&self.inner.state).terminated
    }

    /// 同步读取显式登记实例；待执行或正在执行的装饰不会泄漏旧值。
    pub fn get_instance<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let state = lock(&self.inner.state);
        let service = ServiceKey::typed::<T>();
        let binding = state.bindings.get(&service)?;
        binding.instance.as_ref()?;
        #[cfg(test)]
        binding_event_tests::observe_instance_read();
        Self::ready_value(&state, &service, Some(binding), state.root_scope)
    }

    /// 同步读取成功发布的缓存；未完成的构造或装饰返回 `None`。
    pub fn get_ready<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let state = lock(&self.inner.state);
        let service = ServiceKey::typed::<T>();
        Self::ready_value(
            &state,
            &service,
            state.bindings.get(&service),
            state.root_scope,
        )
    }

    pub(crate) fn get_entry_ready<T: ?Sized + Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let scope = self.current_entry_scope();
        let state = lock(&self.inner.state);
        let scope = match &scope {
            Some(scope) if scope.closed.closed.load(Ordering::Acquire) => return None,
            Some(scope) => scope.id,
            None => state.root_scope,
        };
        let service = state.alias_key(&ServiceKey::typed::<T>()).ok()?;
        Self::ready_value(&state, &service, state.bindings.get(&service), scope)
    }

    fn ready_value<T: ?Sized + Send + Sync + 'static>(
        state: &State,
        service: &ServiceKey,
        binding: Option<&Binding>,
        scope: u64,
    ) -> Option<Arc<T>> {
        if state.terminated {
            return None;
        }
        let scope = match state.service_lifetime(service, binding) {
            Lifetime::Bind => return None,
            Lifetime::Singleton => u64::MAX,
            Lifetime::Scoped => scope,
        };
        let key = CacheKey {
            service: service.clone(),
            scope,
        };
        if state.extensions.pending_operation(&key)
            || binding.is_some_and(|binding| {
                !state
                    .extensions
                    .instance_complete(service, binding.pending.len())
            })
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
        let binding = binding?;
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
        self.root_resolver()?.resolve().await
    }

    /// 在根生命周期按键解析。查找键和请求的 Rust 输出类型分别指定。
    pub async fn resolve_key<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
    ) -> Result<Arc<T>> {
        self.root_resolver()?.resolve_key(key).await
    }

    /// 在根生命周期用调用级参数解析类型。非空参数构造独立实例。
    pub fn resolve_with<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        self.resolve_in(None, parameters, false)
    }

    /// 在根生命周期按具名键或别名进行调用级参数解析。
    pub fn resolve_key_with<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        self.resolve_in(Some(key), parameters, false)
    }

    pub(crate) fn resolve_in<T: ?Sized + Send + Sync + 'static>(
        &self,
        lookup: Option<ServiceKey>,
        parameters: ParameterOverrides,
        entry_scope: bool,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        let container = self.clone();
        Resolver::resolve_from(
            move || {
                let selected = if entry_scope && let Some(scope) = container.current_entry_scope() {
                    Ok(scope.resolver())
                } else {
                    container.root_resolver()
                };
                (selected, container)
            },
            lookup,
            parameters,
        )
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

    /// 清空绑定、别名、共享实例及解析历史，使编译类的绑定声明可重新发现。
    ///
    /// 保留环境、上下文规则、标签、持久装饰器及回调。现有作用域保持开放，
    /// 已取得的实例、原工厂快照及已准入的普通构造仍有效，旧构造不会填入新缓存。
    /// 用户值的析构在状态提交后运行。析构失败时返回错误，清空操作不会回滚。
    pub fn flush(&self) -> Result<()> {
        if self.detach_for_flush(false)?.dispose() {
            return Err(container_error(
                "resource_task_interrupted",
                "容器目录已清空，但旧绑定或实例析构异常",
            ));
        }
        Ok(())
    }

    /// Application 的普通解析回调与重绑监听随目录一起撤销。
    pub(crate) fn detach_for_application_flush(&self) -> Result<FlushedContainer> {
        self.detach_for_flush(true)
    }

    fn detach_for_flush(&self, remove_callbacks: bool) -> Result<FlushedContainer> {
        let mut state = lock(&self.inner.state);
        if state.terminated {
            return Err(container_error("container_closed", "容器已关闭"));
        }
        let revision = state.next_source_revision()?;
        state.next_generation = revision;
        state.flush_revision = revision;
        state.aliases = directory::AliasState::default();
        state.binding_order.clear();
        state.checked_binding_declarations.clear();
        state.class_source_revisions.clear();
        state.resolved_history.clear();
        let (hooks, listeners) = if remove_callbacks {
            (
                std::mem::take(&mut state.hooks),
                state.extensions.take_listeners(),
            )
        } else {
            (ResolveHooks::default(), HashMap::new())
        };
        Ok(FlushedContainer {
            bindings: std::mem::take(&mut state.bindings),
            slots: std::mem::take(&mut state.slots),
            discoveries: std::mem::take(&mut state.binding_discoveries),
            operations: state.extensions.flush_resolved(),
            changes: Arc::clone(&state.binding_changes),
            hooks,
            listeners,
        })
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
            let removed = close_scope(&mut state, old, &resources.registrar());
            (closed, resources, removed)
        };
        // Waker 和用户服务的 Drop 都可能同步重入容器，必须先释放内部锁。
        closed.changed.notify_waiters();
        for attempt in &removed.attempts {
            attempt.changed.notify_waiters();
        }
        dispose_scope(removed);
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
    ///
    /// 首次封闭保留当时在途原工厂的清理证据，分步关闭和取消重试不会丢失失败。
    /// 首次异步关闭的期限跨重试共用；同步用户析构若不返回，无法强制中断。
    pub async fn shutdown_root(&self, deadline: tokio::time::Instant) -> Result<()> {
        let deadline = lock(&self.inner.state)
            .root_raw_close
            .as_ref()
            .and_then(|close| close.deadline)
            .unwrap_or(deadline);
        let _reset = tokio::time::timeout_at(deadline, self.inner.reset.lock())
            .await
            .map_err(|_| container_error("scope_shutdown_timeout", "等待根生命周期关闭超过期限"))?;
        let fences = self.seal_root_snapshot();
        self.compact_root_raw_close();
        let deadline = {
            let mut state = lock(&self.inner.state);
            state
                .root_raw_close
                .as_mut()
                .map_or(deadline, |close| *close.deadline.get_or_insert(deadline))
        };
        let resources = Arc::clone(&lock(&self.inner.state).root_resources);
        let root_result = resources.shutdown(deadline).await;
        let standalone_result = match &self.inner.standalone_resources {
            Some(owner) => owner.shutdown(deadline).await,
            None => Ok(()),
        };
        let mut first_error = lock(&self.inner.state)
            .root_raw_close
            .as_ref()
            .is_some_and(|close| close.interrupted)
            .then(|| container_error("resource_task_interrupted", "首次根封闭中的原工厂清理异常"));
        for fence in fences {
            let result = tokio::time::timeout_at(deadline, fence.wait())
                .await
                .map_err(|_| {
                    container_error("factory_shutdown_timeout", "等待原工厂调用关闭超过共同期限")
                })
                .and_then(std::convert::identity);
            if let Err(error) = result {
                tracing::error!(code = error.code(), "raw factory shutdown fence failed");
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        self.compact_root_raw_close();
        root_result?;
        standalone_result?;
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// 同步封闭根与当前子生命周期；Drop 无法证明异步回收完成。
    pub(crate) fn seal_root(&self) {
        drop(self.seal_root_snapshot());
    }

    fn seal_root_snapshot(&self) -> Vec<crate::foundation::resources::ResourceTaskCompletion> {
        let (closed, resources, removed, termination, fences) = {
            let mut state = lock(&self.inner.state);
            state.terminated = true;
            state.termination.mark_closed();
            let closed = Arc::clone(&state.root_closed);
            closed.mark_closed();
            let resources = Arc::clone(&state.root_resources);
            let termination = Arc::clone(&state.termination);
            if state.root_raw_close.is_none() {
                let pending = state.extensions.descriptor_completions();
                let pending = state
                    .raw_attempts
                    .values()
                    .map(|record| record.completion.clone())
                    .chain(pending);
                state.root_raw_close = Some(RootRawClose {
                    pending: pending.collect(),
                    interrupted: false,
                    deadline: None,
                });
            }
            let fences = state
                .root_raw_close
                .as_ref()
                .map(|close| close.pending.clone())
                .unwrap_or_default();
            let root_scope = state.root_scope;
            let removed = close_scope(&mut state, root_scope, &resources.registrar());
            (closed, resources, removed, termination, fences)
        };
        closed.changed.notify_waiters();
        termination.changed.notify_waiters();
        for attempt in &removed.attempts {
            attempt.changed.notify_waiters();
        }
        dispose_scope(removed);
        resources.seal();
        if let Some(owner) = &self.inner.standalone_resources {
            owner.seal();
        }
        fences
    }

    fn compact_root_raw_close(&self) {
        let removed = {
            let mut state = lock(&self.inner.state);
            let Some(close) = state.root_raw_close.as_mut() else {
                return;
            };
            let mut removed = Vec::new();
            let mut pending = Vec::new();
            for fence in close.pending.drain(..) {
                if let Some(interrupted) = fence.finished_interrupted() {
                    close.interrupted |= interrupted;
                    removed.push(fence);
                } else {
                    pending.push(fence);
                }
            }
            close.pending = pending;
            removed
        };
        drop(removed); // 最后Arc释放不进入State锁。
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
                let on_resources = resources.registrar();
                registrar
                    .track_child(Arc::clone(&resources), move || {
                        if let Some(inner) = inner.upgrade() {
                            close_scope_status(&inner, id, &on_close, &on_resources);
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
            parameters: None,
            previous_parameters: None,
            build_consumer: None,
            method_consumer: None,
            class_builds: Vec::new(),
            binding_checks: Vec::new(),
            factory_parameters: Arc::new(FactoryParameters::empty()),
            factory_completion: None,
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
        use crate::foundation::lifecycle::disposal::{self, Credential};
        if self.closed.closed.load(Ordering::Acquire) {
            let error = container_error("scope_closed", "生命周期已关闭");
            let mut cleanup = self.resources.registrar().begin_cleanup();
            let credential = cleanup
                .as_ref()
                .map(|cleanup| Credential::Cleanup(cleanup.disposal_ticket()));
            let report = disposal::dispose(task, credential);
            if let Some(cleanup) = &mut cleanup {
                cleanup.finish(0);
            }
            if report.seen {
                return Err(crate::foundation::resources::ResourceTaskRejection::error(
                    error,
                    crate::foundation::lifecycle::task_cleanup_interrupted(),
                ));
            }
            return Err(error);
        }
        let scope = Arc::clone(self);
        let failures = self.resources.registrar().task_failures();
        self.resources
            .registrar()
            .spawn(move |cancellation| async move {
                let execution =
                    super::ExecutionContext::new(metadata, deadline, cancellation.clone());
                scope
                    .run(super::with_execution_context(
                        execution,
                        crate::foundation::lifecycle::controlled_task(task, cancellation, failures),
                    ))
                    .await;
            })
    }

    /// 在本生命周期解析服务。
    pub async fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.resolver().resolve().await
    }

    /// 在本生命周期按键解析。别名与类型化入口共享相同的作用域缓存。
    pub async fn resolve_key<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
    ) -> Result<Arc<T>> {
        self.resolver().resolve_key(key).await
    }

    /// 在当前作用域用调用级参数解析类型。
    pub fn resolve_with<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        self.resolver().resolve_with(parameters)
    }

    /// 在当前作用域按键进行调用级参数解析。
    pub fn resolve_key_with<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        self.resolver().resolve_key_with(key, parameters)
    }

    fn resolver(&self) -> Resolver {
        self.container.resolver(
            self.id,
            Arc::clone(&self.closed),
            self.resources.registrar(),
        )
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
        close_scope_status(
            &self.container.inner,
            self.id,
            &self.closed,
            &self.resources.registrar(),
        )
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.close();
    }
}

fn close_scope_status(
    inner: &Arc<Inner>,
    id: u64,
    closed: &Arc<ScopeStatus>,
    resources: &ResourceRegistrar,
) -> bool {
    let mut state = lock(&inner.state);
    if closed.closed.swap(true, Ordering::AcqRel) {
        return false;
    }
    closed.mark_closed();
    let removed = close_scope(&mut state, id, resources);
    drop(state);
    closed.changed.notify_waiters();
    for attempt in &removed.attempts {
        attempt.changed.notify_waiters();
    }
    dispose_scope(removed);
    true
}

/// State移出的两组与其退役fence共同持有；即使外层提前退出也先处置数据再发布。
struct RemovedScope {
    attempts: Vec<Arc<Attempt>>,
    extenders: Vec<Extender>,
    cleanup: Option<crate::foundation::resources::ResourceCleanup>,
}
impl Drop for RemovedScope {
    fn drop(&mut self) {
        use crate::foundation::lifecycle::disposal::{self, Credential};
        let credential = self
            .cleanup
            .as_ref()
            .map(|cleanup| Credential::Cleanup(cleanup.disposal_ticket()));
        let (failures, _) = disposal::scope(credential, || {
            let mut failures = 0;
            for value in std::mem::take(&mut self.attempts) {
                failures += usize::from(disposal::dispose(value, None).owned);
            }
            for value in std::mem::take(&mut self.extenders) {
                failures += usize::from(disposal::dispose(value, None).owned);
            }
            failures
        });
        if let Some(mut cleanup) = self.cleanup.take() {
            cleanup.finish(failures);
        }
    }
}
fn dispose_scope(removed: RemovedScope) {
    drop(removed);
}

fn close_scope(state: &mut State, scope: u64, resources: &ResourceRegistrar) -> RemovedScope {
    let cleanup = resources.begin_cleanup();
    let keys: Vec<_> = state
        .slots
        .keys()
        .filter(|key| key.scope == scope)
        .cloned()
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
                unresolvable: false,
            }));
        }
    }
    let extenders = state.extensions.discard_scope(scope);
    RemovedScope {
        attempts: removed,
        extenders,
        cleanup,
    }
}

struct ActiveFactory(Arc<AtomicBool>);

impl Drop for ActiveFactory {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// 显式查找键用于 alias 路径；输出键和擦除类型描述调用方期望。
enum ResolveRequest {
    Typed {
        lookup: Option<ServiceKey>,
        target: ServiceKey,
        erased_output: TypeId,
    },
    // Laravel getClosure 使用 raiseEvents=false。这个请求只约束具体服务的本层；
    // 类构造器内部另发 Typed 请求，照常参与独立的解析事件。
    Wrapped {
        concrete: ServiceKey,
        target: ServiceKey,
        erased_output: TypeId,
    },
    Erased(ServiceKey),
}

impl ResolveRequest {
    fn raises_events(&self) -> bool {
        !matches!(self, Self::Wrapped { .. })
    }

    fn lookup_key(&self) -> &ServiceKey {
        match self {
            Self::Typed { lookup, target, .. } => lookup.as_ref().unwrap_or(target),
            Self::Wrapped { concrete, .. } | Self::Erased(concrete) => concrete,
        }
    }

    fn target(&self) -> Option<&ServiceKey> {
        match self {
            Self::Typed { target, .. } | Self::Wrapped { target, .. } => Some(target),
            Self::Erased(_) => None,
        }
    }

    fn expected_output(&self) -> Option<TypeId> {
        match self {
            Self::Typed { erased_output, .. } | Self::Wrapped { erased_output, .. } => {
                Some(*erased_output)
            }
            Self::Erased(_) => None,
        }
    }
}

/// 工厂收到的解析上下文；仅活跃构造帧参与环检测，已结束的工厂不会污染延迟解析。
#[derive(Clone)]
pub struct Resolver {
    container: Weak<Inner>,
    scope: u64,
    chain: Vec<(ServiceKey, Arc<AtomicBool>)>,
    constructing: Option<(u64, Arc<AtomicBool>)>,
    captive: bool,
    closed: Arc<ScopeStatus>,
    resources: Option<ResourceRegistrar>,
    parameters: Option<ParameterView>,
    previous_parameters: Option<ParameterView>,
    build_consumer: Option<(ServiceKey, Arc<AtomicBool>)>,
    method_consumer: Option<Arc<MethodConsumer>>,
    class_builds: Vec<ClassBuildFrame>,
    binding_checks: Vec<(ServiceKey, Arc<AtomicBool>)>,
    factory_parameters: Arc<FactoryParameters>,
    factory_completion: Option<crate::foundation::resources::ResourceTaskCompletion>,
}

#[derive(Clone)]
struct ClassBuildFrame {
    key: ServiceKey,
    active: Arc<AtomicBool>,
    self_building: bool,
}

impl Resolver {
    pub(crate) fn belongs_to_http_scope(&self, scope: &Scope) -> bool {
        !self.captive
            && self.scope == scope.id
            && Arc::ptr_eq(&self.closed, &scope.closed)
            && !self.closed.closed.load(Ordering::Acquire)
    }

    /// 从同一弱容器解析当前根服务，不续用已关闭的旧请求作用域。
    ///
    /// 供构造完成的长寿命扩展句柄触发新根操作；活跃工厂内拒绝此入口，
    /// 避免绕过构造环与生命周期约束。不保活应用，根关闭后仍拒绝解析。
    pub async fn resolve_root<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        if self
            .constructing
            .as_ref()
            .is_some_and(|(_, active)| active.load(Ordering::Acquire))
            || self
                .chain
                .iter()
                .any(|(_, active)| active.load(Ordering::Acquire))
        {
            return Err(container_error(
                "root_resolution_during_construction",
                "服务构造期间不能另起根解析上下文",
            ));
        }
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
        let container = self.container()?;
        let state = lock(&container.inner.state);
        self.check_open(&state)?;
        Ok(state.bound_key_at(&ServiceKey::typed::<T>(), self.scope))
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

    /// 先从新调用的Resolver取走旧引用，交给已登记frame的构造清理责任后实际释放。
    fn take_factory_inputs(&mut self) -> PreviousFactoryInputs {
        PreviousFactoryInputs {
            parameters: std::mem::replace(
                &mut self.factory_parameters,
                Arc::new(FactoryParameters::empty()),
            ),
            completion: self.factory_completion.take(),
        }
    }

    fn callback_disposal(&self) -> crate::foundation::lifecycle::CallbackDisposal {
        crate::foundation::lifecycle::CallbackDisposal::new(
            self.factory_completion.clone(),
            self.resources.clone(),
        )
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

    pub(crate) fn container(&self) -> Result<Container> {
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
        self.resolve_lookup::<T>(None)
    }

    /// 用调用级参数解析类型。普通嵌套解析使用自己的空参数集合。
    pub fn resolve_with<T: ?Sized + Send + Sync + 'static>(
        &self,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        let resolver = self.clone();
        Self::resolve_from(move || (Ok(resolver), ()), None, parameters)
    }

    /// 按键用调用级参数解析服务，输出类型与查找键分别指定。
    pub fn resolve_key_with<T: ?Sized + Send + Sync + 'static>(
        &self,
        key: ServiceKey,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        let resolver = self.clone();
        Self::resolve_from(move || (Ok(resolver), ()), Some(key), parameters)
    }

    /// 复制本次工厂或装饰器可见的显式参数。调用结束后的上下文返回空集合。
    pub fn parameters(&self) -> ParameterOverrides {
        if self.factory_parameters.owner.is_some() {
            return if self
                .factory_parameters
                .completion
                .as_ref()
                .and_then(
                    crate::foundation::resources::ResourceTaskCompletion::finished_interrupted,
                )
                .is_some()
            {
                ParameterOverrides::new()
            } else {
                self.factory_parameters.overrides.clone()
            };
        }
        self.parameters
            .as_ref()
            .map_or_else(ParameterOverrides::new, ParameterView::snapshot)
    }

    /// 返回当前仍在选参的编译类。类属性的后置回调发生在本类退出构造栈之后。
    #[must_use]
    pub fn currently_resolving(&self) -> Option<&ServiceKey> {
        self.class_builds
            .iter()
            .rev()
            .find(|frame| frame.active.load(Ordering::Acquire))
            .map(|frame| &frame.key)
    }

    fn class_build(&self, key: &ServiceKey) -> Option<&ClassBuildFrame> {
        self.class_builds
            .iter()
            .rev()
            .find(|frame| &frame.key == key && frame.active.load(Ordering::Acquire))
    }

    fn is_self_building(&self, key: &ServiceKey) -> bool {
        self.class_build(key)
            .is_some_and(|frame| frame.self_building)
    }

    fn suppress_self_building_reentry(&mut self, key: &ServiceKey) {
        if let Some(frame) = self
            .class_builds
            .iter_mut()
            .rev()
            .find(|frame| &frame.key == key && frame.active.load(Ordering::Acquire))
        {
            frame.self_building = false;
        }
    }

    fn active_consumer_frame(&self) -> Option<&(ServiceKey, Arc<AtomicBool>)> {
        self.build_consumer
            .as_ref()
            .filter(|(_, active)| active.load(Ordering::Acquire))
    }

    fn for_new_lookup(mut self) -> Self {
        self.previous_parameters = self.parameters.take();
        self.clear_factory_inputs();
        self
    }

    fn clear_factory_inputs(&mut self) {
        self.factory_parameters = Arc::new(FactoryParameters::empty());
        self.factory_completion = None;
    }

    fn resolve_from<T: ?Sized + Send + Sync + 'static, K: Send + 'static>(
        resolver: impl FnOnce() -> (Result<Self>, K) + Send + 'static,
        lookup: Option<ServiceKey>,
        parameters: ParameterOverrides,
    ) -> BoxFuture<'static, Result<Arc<T>>> {
        let request = ResolveRequest::Typed {
            lookup,
            target: ServiceKey::typed::<T>(),
            erased_output: TypeId::of::<Arc<T>>(),
        };
        Self::resolve_mapped(resolver, request, parameters, |value| {
            value.downcast_ref::<Arc<T>>().cloned().ok_or_else(|| {
                container_error("service_type_mismatch", "服务的实际输出不符合声明类型")
            })
        })
    }

    fn resolve_mapped<T: Send + 'static, K: Send + 'static>(
        resolver: impl FnOnce() -> (Result<Self>, K) + Send + 'static,
        request: ResolveRequest,
        parameters: ParameterOverrides,
        deliver: fn(Value) -> Result<T>,
    ) -> BoxFuture<'static, Result<T>> {
        use crate::foundation::lifecycle::CallbackDisposal;

        // 未轮询的调用尚未选定作用域，仍持有可受控退役的参数。
        let frame = (!parameters.is_empty())
            .then(|| ParameterFrame::new(parameters, CallbackDisposal::new(None, None)));
        Box::pin(async move {
            // 选择器可交回强持有者，保证延迟轮询的根容器活到结果交付。
            let (selected, _keepalive) = resolver();
            let resolver = match selected {
                Ok(resolver) => resolver.for_new_lookup(),
                Err(error) => {
                    return match frame {
                        Some(frame) => frame.finish(Err(error)),
                        None => Err(error),
                    };
                }
            };
            deliver(resolver.resolve_with_frame(request, frame).await?)
        })
    }

    async fn resolve_with_frame(
        mut self,
        request: ResolveRequest,
        mut frame: Option<ParameterFrame>,
    ) -> Result<Value> {
        if let Some(frame) = &mut frame {
            frame.set_disposal(self.callback_disposal());
            self.parameters = Some(frame.view());
        }
        let result = self.resolve_projected(request, None).await;
        match frame {
            Some(frame) => self.deliver_after_parameters(frame.finish(result)?),
            None => result,
        }
    }

    fn deliver_after_parameters(&self, value: Value) -> Result<Value> {
        if let Some(inner) = self.container.upgrade() {
            return self.deliver_output(&inner, value, None);
        }
        let primary = container_error("container_closed", "容器已释放");
        self.callback_disposal()
            .run(|| Err(Self::retire_rejected_output(value, primary, false)))
    }

    #[cfg(test)]
    fn resolve_value(&self, key: ServiceKey) -> BoxFuture<'_, Result<Value>> {
        let mut child = self.clone();
        child.factory_parameters = Arc::new(FactoryParameters::empty());
        child.factory_completion = None;
        Box::pin(async move { child.resolve_at_generation(key, None).await })
    }

    fn dependency_path(&self, key: &ServiceKey) -> String {
        self.chain
            .iter()
            .filter(|(_, active)| active.load(Ordering::Acquire))
            .map(|(parent, _)| parent.to_string())
            .chain(std::iter::once(key.to_string()))
            .collect::<Vec<_>>()
            .join(" → ")
    }

    fn resolve_at_generation(
        &self,
        key: ServiceKey,
        expected: Option<u64>,
    ) -> BoxFuture<'_, Result<Value>> {
        self.resolve_projected(ResolveRequest::Erased(key), expected)
    }

    fn missing_service(&self, key: &ServiceKey) -> Error {
        unresolvable_dependency(
            "service_not_bound",
            format!("未绑定服务：{}", self.dependency_path(key)),
        )
    }

    fn binding_source(
        &self,
        state: &State,
        requested: &ServiceKey,
        expected: Option<u64>,
        pinned: bool,
    ) -> Result<descriptor_source::ConstructionSource> {
        use descriptor_source::ConstructionSource;
        self.check_open(state)?;
        let canonical = state.classes.canonical_key(requested);
        let aliases = if expected.is_none() {
            Some(state.aliases.lookup_witness(&canonical, pinned)?)
        } else {
            None
        };
        let key = aliases
            .as_ref()
            .map_or(canonical, |path| path.terminal.clone());
        if let Some(generation) = expected {
            let entry = state.bindings.get(&key);
            if entry.is_none()
                && let Some(binding) = state.implicit_class_binding(&key)
                && binding.generation == generation
            {
                return Ok(ConstructionSource::Binding(BindingSource::implicit(
                    key, binding, None,
                )));
            }
            if entry.map(|binding| binding.generation) != expected {
                return Err(container_error(
                    "binding_replaced",
                    "重绑定事件对应的服务已被替换",
                ));
            }
        }
        Ok(
            match state.resolution_binding(&key, self.parameters.is_some()) {
                Some(binding) => ConstructionSource::Binding(BindingSource {
                    key,
                    binding,
                    aliases,
                    contextual: None,
                    implicit: false,
                }),
                None => ConstructionSource::Missing(key),
            },
        )
    }

    async fn select_resolution(
        &self,
        inner: &Arc<Inner>,
        request: &ResolveRequest,
        expected: Option<u64>,
        pinned: Option<&ServiceKey>,
    ) -> Result<std::ops::ControlFlow<Value, BindingSource>> {
        let requested = pinned.unwrap_or_else(|| request.lookup_key());
        let mut checked = None;
        loop {
            use descriptor_source::ConstructionSource;
            let choice = 'select: {
                let mut state = lock(&inner.state);
                let contextual = if expected.is_none() && self.active_consumer_frame().is_some() {
                    state.contextual_source(requested, self, request, pinned.is_some())?
                } else {
                    None
                };
                let revision = state.flush_revision;
                let checked_here = checked
                    .as_ref()
                    .filter(|(checked_revision, _)| *checked_revision == revision)
                    .map(|(_, key)| key);
                if contextual.is_none()
                    && expected.is_none()
                    && let Some(declared) = self.begin_binding_discovery(
                        inner,
                        &mut state,
                        requested,
                        pinned.is_some(),
                        checked_here,
                    )?
                {
                    break 'select std::ops::ControlFlow::Break((revision, declared));
                }
                let selected = match contextual {
                    Some(source) => source,
                    None if expected.is_none() && state.classes.has_descriptors() => {
                        state.construction_source(requested, self, pinned.is_some())?
                    }
                    None => self.binding_source(&state, requested, expected, pinned.is_some())?,
                };
                #[cfg(test)]
                if matches!(&selected, ConstructionSource::Binding(_)) {
                    directory_tests_v2::observe_selected();
                }
                std::ops::ControlFlow::Continue(selected)
            };
            let source = match choice {
                std::ops::ControlFlow::Continue(source) => source,
                std::ops::ControlFlow::Break((revision, declared)) => {
                    checked = declared
                        .resolve(inner, self)
                        .await?
                        .map(|key| (revision, key));
                    continue;
                }
            };
            match source {
                ConstructionSource::Binding(source) => {
                    return Ok(std::ops::ControlFlow::Continue(source));
                }
                ConstructionSource::Missing(key) => return Err(self.missing_service(&key)),
                ConstructionSource::Descriptor(source) => {
                    let output = request
                        .target()
                        .ok_or_else(|| self.missing_service(source.key()))?;
                    let erased_output = request
                        .expected_output()
                        .ok_or_else(|| self.missing_service(source.key()))?;
                    if let Some(value) = self
                        .resolve_descriptor(
                            inner,
                            &source,
                            output,
                            erased_output,
                            request.raises_events(),
                        )
                        .await?
                    {
                        return Ok(std::ops::ControlFlow::Break(value));
                    }
                }
            }
        }
    }

    fn binding_projection(
        &self,
        inner: &Inner,
        source: &BindingSource,
        request: &ResolveRequest,
    ) -> Result<(Option<ClassProjection>, bool)> {
        let state = lock(&inner.state);
        self.check_open(&state)?;
        if !source.is_current(&state) {
            return Err(container_error(
                "binding_replaced",
                "解析准入前服务来源已更换",
            ));
        }
        let reentry = self.is_self_building(&source.key)
            && source
                .binding
                .class_factory
                .as_ref()
                .is_some_and(|factory| factory.builds_directly());
        if !reentry
            && self
                .chain
                .iter()
                .any(|(id, active)| *id == source.key && active.load(Ordering::Acquire))
        {
            return Err(container_error(
                "service_cycle",
                self.dependency_path(&source.key),
            ));
        }
        let projection = source.binding.output_projection(&state.classes, request)?;
        if source.binding.effective_lifetime() == Lifetime::Scoped && self.captive {
            return Err(container_error(
                "captive_scope_dependency",
                format!("singleton 不能捕获 scoped 服务 `{}`", source.key),
            ));
        }
        let nested_uncached = reentry
            && source.binding.instance.is_none()
            && match source.binding.effective_lifetime() {
                Lifetime::Bind => true,
                lifetime => {
                    let scope = if lifetime == Lifetime::Singleton {
                        u64::MAX
                    } else {
                        self.scope
                    };
                    let key = CacheKey {
                        service: source.key.clone(),
                        scope,
                    };
                    state.slots.get(&key).is_some_and(|attempt| {
                        lock(&attempt.outcome).is_none()
                            && self.constructing.as_ref().is_some_and(|(current, active)| {
                                active.load(Ordering::Acquire)
                                    && (*current == attempt.id
                                        || state.path(attempt.id, *current).is_some())
                            })
                    })
                }
            };
        Ok((projection, nested_uncached))
    }

    async fn before_resolution(
        &self,
        inner: &Arc<Inner>,
        request: &ResolveRequest,
        expected: Option<u64>,
    ) -> Result<Option<ServiceKey>> {
        if !request.raises_events() {
            return Ok(None);
        }
        let key = {
            let state = lock(&inner.state);
            self.check_open(&state)?;
            if !state.hooks.has_before() {
                return Ok(None);
            }
            let key = state.classes.canonical_key(request.lookup_key());
            if expected.is_some() {
                key
            } else {
                state.aliases.resolve(&key)?
            }
        };
        if self
            .chain
            .iter()
            .any(|(parent, active)| parent == &key && active.load(Ordering::Acquire))
            && !self.is_self_building(&key)
        {
            return Err(container_error("service_cycle", self.dependency_path(&key)));
        }
        let mut child = self.clone();
        let active = Arc::new(AtomicBool::new(true));
        let _guard = ActiveFactory(Arc::clone(&active));
        child.chain.push((key.clone(), active));
        // 同键 before 回调不能再次使用外层自构造的重入许可。
        child.suppress_self_building_reentry(&key);
        // Laravel 先把本次显式实参交给 before，再压入构造覆盖栈。
        child.parameters = child.previous_parameters.take();
        let fired = child
            .fire_before(inner, &key, self.parameters.clone())
            .await?;
        Ok(fired.then_some(key))
    }

    fn resolve_projected(
        &self,
        request: ResolveRequest,
        expected: Option<u64>,
    ) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let container = self.container()?;
            let pinned = self
                .before_resolution(&container.inner, &request, expected)
                .await?;
            loop {
                let mut source = match self
                    .select_resolution(&container.inner, &request, expected, pinned.as_ref())
                    .await?
                {
                    std::ops::ControlFlow::Continue(binding) => binding,
                    std::ops::ControlFlow::Break(value) => return Ok(value),
                };
                #[cfg(test)]
                directory_tests_v2::after_binding_selection().await;
                let (projection, nested_uncached) =
                    match self.binding_projection(&container.inner, &source, &request) {
                        Ok(projection) => projection,
                        Err(error) if error.code() == "binding_replaced" => continue,
                        Err(error) => return Err(error),
                    };
                if !source.binding.pending.is_empty() {
                    match self.initialize_instance(&container.inner, &source).await {
                        Ok(true) => source.aliases = None,
                        Ok(false) => {}
                        Err(error) if error.code() == "binding_replaced" => continue,
                        Err(error) => return Err(error),
                    }
                }
                let mut child = self.clone();
                child
                    .chain
                    .retain(|(_, active)| active.load(Ordering::Acquire));
                let activity = Arc::new(AtomicBool::new(true));
                let _activity = ActiveFactory(Arc::clone(&activity));
                child
                    .chain
                    .push((source.key.clone(), Arc::clone(&activity)));
                let lifetime = source.binding.effective_lifetime();
                if lifetime == Lifetime::Singleton {
                    child.captive = true;
                    child.scope = u64::MAX;
                    child.closed = Arc::new(ScopeStatus::default());
                    child.resources = container.inner.resources.clone();
                }
                // 自构造的重入使用普通构造。外层在途共享尝试仍负责最终发布。
                let value = if lifetime == Lifetime::Bind || nested_uncached {
                    self.transient(&source, child, activity, request.raises_events())
                        .await?
                } else {
                    self.cached(&source, child, activity, request.raises_events())
                        .await?
                };
                if let Some(value) = value {
                    return self.deliver_output(&container.inner, value, projection.as_ref());
                }
            }
        })
    }

    fn deliver_output(
        &self,
        inner: &Inner,
        value: Value,
        projection: Option<&ClassProjection>,
    ) -> Result<Value> {
        use crate::foundation::lifecycle::{TaskExecutionFailure, catch_isolated, disposal};
        let open = self.check_open(&lock(&inner.state));
        if let Err(primary) = open {
            return self
                .callback_disposal()
                .run(|| Err(Self::retire_rejected_output(value, primary, false)));
        }
        let Some(projection) = projection else {
            return Ok(value);
        };
        self.callback_disposal().run(|| {
            let open = self.check_open(&lock(&inner.state));
            if let Err(primary) = open {
                return Err(Self::retire_rejected_output(value, primary, false));
            }
            let result = if let Ok(result) = catch_isolated(|| projection.project(&value)) {
                result
            } else {
                disposal::failure(None);
                Err(container_error(
                    "resource_task_interrupted",
                    "输出视图转换异常",
                ))
            };

            let cleanup_failed = disposal::dispose(value, None).seen;
            let open = self.check_open(&lock(&inner.state));
            if let Err(primary) = open {
                return Err(Self::retire_rejected_output(
                    result,
                    primary,
                    cleanup_failed,
                ));
            }
            if !cleanup_failed {
                return result;
            }
            let cleanup = container_error("resource_task_interrupted", "输出视图清理异常");
            match result {
                Ok(value) => {
                    disposal::dispose(value, None);
                    Err(cleanup)
                }
                Err(primary) => Err(TaskExecutionFailure::error(primary, cleanup)),
            }
        })
    }

    fn retire_rejected_output<T>(output: T, primary: Error, cleanup_failed: bool) -> Error {
        use crate::foundation::lifecycle::{TaskExecutionFailure, disposal};
        let rejected_cleanup_failed = disposal::dispose(output, None).seen;
        if cleanup_failed || rejected_cleanup_failed {
            TaskExecutionFailure::error(
                primary,
                container_error("resource_task_interrupted", "拒绝交付的输出清理异常"),
            )
        } else {
            primary
        }
    }

    async fn cached(
        &self,
        source: &BindingSource,
        mut child: Self,
        activity: Arc<AtomicBool>,
        events: bool,
    ) -> Result<Option<Value>> {
        let container = self.container()?;
        let service = &source.key;
        let binding = &source.binding;
        // singleton 不能和 root scoped 共用一个缓存键。
        let key = CacheKey {
            service: service.clone(),
            scope: if binding.effective_lifetime() == Lifetime::Singleton {
                u64::MAX
            } else {
                self.scope
            },
        };
        let Some(CachedAdmission {
            attempt,
            owner_chain,
            edge: _edge,
        }) = self.admit_cached(&container.inner, &key, source).await?
        else {
            return Ok(None);
        };
        let Some(chain) = owner_chain else {
            return tokio::select! {
                result = attempt.wait() => result.map(Some),
                () = self.closed.wait_closed() => Err(container_error("scope_closed", "生命周期已关闭")),
            };
        };
        let mut guard = Construction {
            inner: Arc::clone(&container.inner),
            key: key.clone(),
            attempt: Arc::clone(&attempt),
            activity,
            finished: false,
        };
        child.constructing = child
            .chain
            .last()
            .map(|(_, active)| (attempt.id, Arc::clone(active)));
        let outcome = tokio::select! {
            result = child.construct_value(binding, chain) => result,
            closed = attempt.wait() => return closed.map(Some),
            () = self.closed.wait_closed() => return Err(container_error("scope_closed", "生命周期已关闭")),
        };
        match outcome {
            Ok(value) => {
                let run_hooks = events
                    && binding.instance.is_none()
                    && lock(&container.inner.state).hooks.has_completed();
                guard.publish(Ok(Arc::clone(&value)), !run_hooks);
                let mut value = match attempt.wait().await {
                    Ok(published) => published,
                    Err(primary) => {
                        return self
                            .callback_disposal()
                            .run(|| Err(Self::retire_rejected_output(value, primary, false)));
                    }
                };
                if run_hooks {
                    // 完成回调可以读刚发布的共享对象。单例仍使用应用资源与捕获保护，
                    // 同时监听发起解析的作用域关闭。
                    child.closed.clone_from(&self.closed);
                    child.suppress_self_building_reentry(service);
                    value = child
                        .complete_hook_output(&container.inner, service, value)
                        .await?;
                    guard.record_success_if_current(self);
                }
                if !binding.pending.is_empty() {
                    let event = {
                        let state = lock(&container.inner.state);
                        state
                            .bindings
                            .get(service)
                            .filter(|current| current.generation == binding.generation)
                            .and_then(|current| {
                                Rebound::capture(
                                    &state,
                                    &container.inner,
                                    self,
                                    service,
                                    current,
                                    Some(Arc::clone(&value)),
                                )
                            })
                    };
                    if let Some(event) = event {
                        event.dispatch().await?;
                    }
                }
                Ok(Some(value))
            }
            Err(error) => {
                let failure = Failure::initialization(service, error);
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
            .get(&ServiceKey::typed::<usize>())
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
                &BindingSource::pinned(ServiceKey::typed::<usize>(), old),
                resolver.clone(),
                Arc::new(AtomicBool::new(true)),
                true,
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
                            service: ServiceKey::typed::<Lazy>(),
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
                    service: ServiceKey::typed::<Lazy>(),
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
        let activity = Arc::clone(&lock(&saved).as_ref().unwrap().chain.last().unwrap().1);
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

#[cfg(test)]
#[path = "container/root_resolver_tests.rs"]
mod root_resolver_tests;

#[cfg(test)]
#[path = "container/binding_state_tests.rs"]
mod binding_state_tests;

#[cfg(test)]
#[path = "container/binding_event_tests.rs"]
mod binding_event_tests;

#[cfg(test)]
#[path = "container/binding_admission_tests.rs"]
mod binding_admission_tests;

#[cfg(test)]
#[path = "../tests/unit/container/flush.rs"]
mod flush_tests;

#[cfg(test)]
#[path = "container/directory_tests.rs"]
mod directory_tests;

#[cfg(test)]
#[path = "container/directory_tests_v2.rs"]
mod directory_tests_v2;

#[cfg(test)]
#[path = "container/directory_tests_v3.rs"]
mod directory_tests_v3;
#[cfg(test)]
mod directory_tests_v4;
#[cfg(test)]
mod directory_tests_v5;
#[cfg(test)]
mod directory_tests_v6;
#[cfg(test)]
mod directory_tests_v7;

#[cfg(test)]
mod directory_tests_v8;

#[cfg(test)]
mod directory_tests_v9;

#[cfg(test)]
mod directory_tests_v10;

mod class_descriptor;
mod class_projection;

#[cfg(test)]
mod selection_tests;

#[cfg(test)]
#[path = "../tests/unit/container/environment_binding.rs"]
mod environment_binding_tests;

#[cfg(test)]
#[path = "../tests/unit/container/conditional_binding.rs"]
mod conditional_binding_tests;

#[cfg(test)]
#[path = "../tests/unit/container/class_lifetime.rs"]
mod class_lifetime_tests;

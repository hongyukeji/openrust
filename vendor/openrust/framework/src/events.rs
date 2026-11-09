//! 显式类身份、统一具名选择器及响应的进程内事件。
//!
//! exact 组先执行，随后按 wildcard 模式首次登记次序执行各组。类和同名字符串
//! 使用相同选择器；载荷只在到达类型化回调时检查。队列映射在其监听器位置执行。
//! 当前只支持单参数载荷和立即响应；接口投影、延迟及后提交派发仍未实现。

use crate::{
    container::ServiceKey,
    core::{Error, Result},
    database::DatabaseTransaction,
    foundation::{ProviderId, RegisterContext},
    support::{BoxFuture, DataValue},
};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Mutex, MutexGuard},
};

/// 类型所有者声明的稳定事件类身份；不是 Rust 诊断类型名。
///
/// 同一派发器已观察到的类名只能对应一个实际类型。名字不可为空或含 `*`。
/// 字符串派发不声明类身份，也不受这两个类名限制。
///
/// ```
/// # async fn example() -> openrust::Result<()> {
/// use openrust::{events::{Dispatcher, Event, EventServiceProvider, EventsServiceProvider}, support::DataValue};
/// struct Changed;
/// impl Event for Changed { const NAME: &'static str = r"App\Events\Changed"; }
/// let mut provider = EventsServiceProvider::new();
/// provider.listen_reply::<Changed, _>(|_, context| {
///     assert_eq!(context.event_name(), Changed::NAME);
///     Box::pin(async { Ok(DataValue::Bool(true)) })
/// });
/// let mut builder = openrust::foundation::ProviderBuilder::new();
/// builder.provider(EventServiceProvider).provider(provider);
/// let runtime = builder.start(figment::Figment::new()).await?;
/// let events = runtime.container().resolve::<Dispatcher>().await?;
/// assert!(matches!(events.until(&Changed).await?, DataValue::Bool(true)));
/// runtime.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1)).await?;
/// # Ok(()) }
/// ```
pub trait Event: Any + Send + Sync {
    /// 类事件的完整稳定名称。
    const NAME: &'static str;
}

/// 借用本次真实单参数；多个事件可共享载荷类型，但不因此共享事件身份。
///
/// ```
/// use openrust::{events::EventsServiceProvider, support::DataValue};
/// let mut provider = EventsServiceProvider::new();
/// provider.listen_named_reply("messages.*", |payload, context| {
///     let text = payload.downcast_ref::<String>();
///     assert!(context.event_name().starts_with("messages."));
///     Box::pin(async move { Ok(text.map_or(DataValue::Null, |text| DataValue::String(text.clone()))) })
/// });
/// ```
#[derive(Clone, Copy)]
pub struct PayloadRef<'a>(&'a (dyn Any + Send + Sync));
impl<'a> PayloadRef<'a> {
    /// 安全投影实际载荷；不匹配时返回 `None`，不复制或序列化对象。
    #[must_use]
    pub fn downcast_ref<T: Any>(&self) -> Option<&'a T> {
        self.0.downcast_ref()
    }
}

/// 本次派发解析出的名称和调用者借用事务。
#[derive(Clone, Copy)]
pub struct EventContext<'a> {
    transaction: Option<&'a DatabaseTransaction>,
    name: &'a str,
}
impl<'a> EventContext<'a> {
    /// 调用者提供的事务；派发器不负责创建或提交它。
    #[must_use]
    pub const fn transaction(&self) -> Option<&'a DatabaseTransaction> {
        self.transaction
    }
    /// 本次类或字符串派发的完整名称。
    #[must_use]
    pub const fn event_name(&self) -> &'a str {
        self.name
    }
}

/// 只控制传播；停止不表示触发业务或模型保存被否决。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventPropagation {
    /// 继续传播。
    Continue,
    /// 停止传播，保留此前响应。
    Stop,
}

enum Invocation {
    Response(DataValue),
    Stop,
}
trait ErasedListener: Send + Sync {
    fn selector(&self) -> &str;
    fn declaration(&self) -> Option<(&'static str, TypeId)> {
        None
    }
    fn is_queued(&self) -> bool {
        false
    }
    fn call<'a>(
        &'a self,
        event: PayloadRef<'a>,
        context: EventContext<'a>,
    ) -> BoxFuture<'a, Result<Invocation>>;
}
fn mismatch() -> Error {
    configuration_error("event_type_mismatch", "事件监听器的载荷类型不匹配")
}
fn configuration_error(code: &str, message: &str) -> Error {
    Error::custom_with_status(axum::http::StatusCode::INTERNAL_SERVER_ERROR, code, message)
}
fn observe(
    names: &mut HashMap<&'static str, TypeId>,
    name: &'static str,
    identity: TypeId,
) -> Result<()> {
    if name.is_empty() || name.contains('*') {
        return Err(configuration_error(
            "event_name_invalid",
            "类事件名称不可为空或包含通配符",
        ));
    }
    if let Some(previous) = names.get(name) {
        if *previous != identity {
            return Err(configuration_error(
                "event_name_conflict",
                "类事件名称已由其他类型声明",
            ));
        }
    } else {
        names.insert(name, identity);
    }
    Ok(())
}
struct TypedListener<E, F> {
    handler: F,
    marker: PhantomData<fn(&E)>,
}
impl<E, F> ErasedListener for TypedListener<E, F>
where
    E: Event,
    F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<DataValue>> + Send + Sync,
{
    fn selector(&self) -> &str {
        E::NAME
    }
    fn declaration(&self) -> Option<(&'static str, TypeId)> {
        Some((E::NAME, TypeId::of::<E>()))
    }
    fn call<'a>(
        &'a self,
        event: PayloadRef<'a>,
        context: EventContext<'a>,
    ) -> BoxFuture<'a, Result<Invocation>> {
        Box::pin(async move {
            let event = event.downcast_ref::<E>().ok_or_else(mismatch)?;
            (self.handler)(event, context)
                .await
                .map(Invocation::Response)
        })
    }
}
struct NamedListener<F> {
    pattern: String,
    handler: F,
}
impl<F> ErasedListener for NamedListener<F>
where
    F: for<'a> Fn(PayloadRef<'a>, EventContext<'a>) -> BoxFuture<'a, Result<Invocation>>
        + Send
        + Sync,
{
    fn selector(&self) -> &str {
        &self.pattern
    }
    fn call<'a>(
        &'a self,
        event: PayloadRef<'a>,
        context: EventContext<'a>,
    ) -> BoxFuture<'a, Result<Invocation>> {
        (self.handler)(event, context)
    }
}
type Listener = Arc<dyn ErasedListener>;
pub(crate) type DeferredEventHook =
    Arc<dyn Fn() -> BoxFuture<'static, Result<DataValue>> + Send + Sync>;

struct ProviderListener {
    selector: &'static str,
    hook: DeferredEventHook,
}

impl ErasedListener for ProviderListener {
    fn selector(&self) -> &str {
        self.selector
    }

    fn call<'a>(
        &'a self,
        _: PayloadRef<'a>,
        _: EventContext<'a>,
    ) -> BoxFuture<'a, Result<Invocation>> {
        let pending = (self.hook)();
        Box::pin(async move { pending.await.map(Invocation::Response) })
    }
}

#[must_use]
pub(crate) struct RetiredDeferredListeners(Vec<Listener>);

impl RetiredDeferredListeners {
    pub(crate) fn dispose(self) -> bool {
        dispose_listeners(self.0)
    }
}

fn dispose_listeners(listeners: Vec<Listener>) -> bool {
    let mut failed = false;
    for listener in listeners {
        failed |= crate::foundation::lifecycle::disposal::dispose(listener, None).seen;
    }
    failed
}

#[derive(Default)]
pub(crate) struct DeferredEventListeners(Mutex<DeferredEventDirectory>);

#[derive(Default)]
struct DeferredEventDirectory {
    entries: indexmap::IndexMap<ProviderId, (usize, Vec<Listener>)>,
    next: usize,
}

#[derive(Clone, Default)]
pub(crate) struct DeferredEventHooks {
    directory: Arc<DeferredEventListeners>,
}

impl DeferredEventListeners {
    fn directory(&self) -> MutexGuard<'_, DeferredEventDirectory> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn hooks(self: &Arc<Self>) -> DeferredEventHooks {
        DeferredEventHooks {
            directory: Arc::clone(self),
        }
    }

    pub(crate) fn replace(
        &self,
        id: ProviderId,
        selectors: &[&'static str],
        hook: &DeferredEventHook,
    ) -> RetiredDeferredListeners {
        let listeners: Vec<Listener> = selectors
            .iter()
            .map(|&selector| {
                Arc::new(ProviderListener {
                    selector,
                    hook: Arc::clone(hook),
                }) as Listener
            })
            .collect();
        let mut directory = self.directory();
        if let Some((_, current)) = directory.entries.get_mut(&id) {
            return RetiredDeferredListeners(std::mem::replace(current, listeners));
        }
        if !listeners.is_empty() {
            let position = directory.next;
            directory.entries.insert(id, (position, listeners));
            directory.next += 1;
        }
        RetiredDeferredListeners(Vec::new())
    }
}

impl Drop for DeferredEventListeners {
    fn drop(&mut self) {
        let directory = self
            .0
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (_, listeners) in std::mem::take(&mut directory.entries).into_values() {
            let _ = RetiredDeferredListeners(listeners).dispose();
        }
    }
}

impl DeferredEventHooks {
    fn reserve_regular_position(&self) -> usize {
        let mut directory = self.directory.directory();
        let position = directory.next;
        directory.next += 1;
        position
    }

    fn snapshot(&self, mut regular: Option<&RegularListeners>) -> Option<Vec<Listener>> {
        let directory = self.directory.directory();
        if directory.entries.is_empty() {
            return None;
        }
        let mut listeners = Vec::new();
        for (position, group) in directory.entries.values() {
            if let Some(regular) = regular.take_if(|regular| regular.position < *position) {
                listeners.extend_from_slice(&regular.listeners);
            }
            listeners.extend_from_slice(group);
        }
        if let Some(regular) = regular {
            listeners.extend_from_slice(&regular.listeners);
        }
        Some(listeners)
    }
}

// 对应命名事件的完整匹配：只有 * 是通配符，其他字符（包括 ?）按字面值处理。
fn wildcard_name_matches(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.as_bytes(), name.as_bytes());
    let (mut cursor, mut position, mut retry) = (0, 0, 0);
    let mut star = None;
    while position < name.len() {
        if cursor < pattern.len() && pattern[cursor] == b'*' {
            star = Some(cursor);
            cursor += 1;
            retry = position;
        } else if cursor < pattern.len() && pattern[cursor] == name[position] {
            cursor += 1;
            position += 1;
        } else if let Some(previous) = star {
            retry += 1;
            position = retry;
            cursor = previous + 1;
        } else {
            return false;
        }
    }
    pattern[cursor..].iter().all(|byte| *byte == b'*')
}

#[derive(Clone)]
enum ListenerRegistration {
    Direct(Arc<dyn ErasedListener>),
    #[cfg(feature = "queue")]
    Queued {
        declaration: (&'static str, TypeId),
        build: Arc<dyn Fn(crate::queue::Queue) -> Arc<dyn ErasedListener> + Send + Sync>,
    },
}
impl ListenerRegistration {
    fn declaration(&self) -> Option<(&'static str, TypeId)> {
        match self {
            Self::Direct(listener) => listener.declaration(),
            #[cfg(feature = "queue")]
            Self::Queued { declaration, .. } => Some(*declaration),
        }
    }
}
#[derive(Default)]
struct ListenerRegistrations {
    listeners: Vec<ListenerRegistration>,
}

/// 向唯一应用事件服务登记监听器。
///
/// 应用默认清单会登记 [`EventServiceProvider`]。直接组装 Provider 时需要显式登记该核心服务。
#[derive(Default)]
pub struct EventsServiceProvider {
    registrations: ListenerRegistrations,
}
impl EventsServiceProvider {
    /// 创建空 Provider。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// 登记类事件；成功的 unit 响应适配为 null。
    pub fn listen<E, F>(&mut self, handler: F) -> &mut Self
    where
        E: Event,
        F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<()>> + Send + Sync + 'static,
    {
        self.listen_reply::<E, _>(move |event, context| {
            let pending = handler(event, context);
            Box::pin(async move {
                pending.await?;
                Ok(DataValue::Null)
            })
        })
    }
    /// 登记类事件响应；与该类名的字符串 exact 监听共用同一组。
    pub fn listen_reply<E, F>(&mut self, handler: F) -> &mut Self
    where
        E: Event,
        F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<DataValue>>
            + Send
            + Sync
            + 'static,
    {
        self.registrations
            .listeners
            .push(ListenerRegistration::Direct(Arc::new(TypedListener::<
                E,
                F,
            > {
                handler,
                marker: PhantomData,
            })));
        self
    }
    /// 登记具有载荷类型前提的 exact 或 wildcard；到达回调时才下转。
    ///
    /// `Stop` 仅停止传播，不生成 false 响应或业务取消。
    pub fn listen_named<E, F>(&mut self, pattern: impl Into<String>, handler: F) -> &mut Self
    where
        E: Any + Send + Sync + 'static,
        F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<EventPropagation>>
            + Send
            + Sync
            + 'static,
    {
        self.push_named(pattern.into(), move |payload, context| {
            let Some(event) = payload.downcast_ref::<E>() else {
                return Box::pin(async { Err(mismatch()) });
            };
            let pending = handler(event, context);
            Box::pin(async move {
                Ok(match pending.await? {
                    EventPropagation::Continue => Invocation::Response(DataValue::Null),
                    EventPropagation::Stop => Invocation::Stop,
                })
            })
        });
        self
    }
    /// 登记不预限类型的具名响应；同一个 wildcard 可观察不同事件类。
    pub fn listen_named_reply<F>(&mut self, pattern: impl Into<String>, handler: F) -> &mut Self
    where
        F: for<'a> Fn(PayloadRef<'a>, EventContext<'a>) -> BoxFuture<'a, Result<DataValue>>
            + Send
            + Sync
            + 'static,
    {
        self.push_named(pattern.into(), move |payload, context| {
            let pending = handler(payload, context);
            Box::pin(async move { pending.await.map(Invocation::Response) })
        });
        self
    }
    fn push_named<F>(&mut self, pattern: String, handler: F)
    where
        F: for<'a> Fn(PayloadRef<'a>, EventContext<'a>) -> BoxFuture<'a, Result<Invocation>>
            + Send
            + Sync
            + 'static,
    {
        self.registrations
            .listeners
            .push(ListenerRegistration::Direct(Arc::new(NamedListener {
                pattern,
                handler,
            })));
    }
    /// 在监听器到达位置映射拥有型任务；事务内派发使用同一事务入队。
    #[cfg(feature = "queue")]
    pub fn listen_queued<E, J, F>(&mut self, queue_name: impl Into<String>, map: F) -> &mut Self
    where
        E: Event,
        J: crate::queue::Job + serde::Serialize,
        F: Fn(&E) -> Result<J> + Send + Sync + 'static,
    {
        let queue_name = queue_name.into();
        let map = Arc::new(map);
        self.registrations
            .listeners
            .push(ListenerRegistration::Queued {
                declaration: (E::NAME, TypeId::of::<E>()),
                build: Arc::new(move |queue| {
                    Arc::new(QueuedListener::<E, J, F> {
                        queue,
                        queue_name: queue_name.clone(),
                        map: Arc::clone(&map),
                        marker: PhantomData,
                    })
                }),
            });
        self
    }
}
impl crate::support::ServiceProvider for EventsServiceProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut declarations = HashMap::new();
            for registration in &self.registrations.listeners {
                if let Some((name, identity)) = registration.declaration() {
                    observe(&mut declarations, name, identity)?;
                }
            }
            let hooks = app.deferred_event_hooks();
            let position = hooks.reserve_regular_position();
            let registrations = Arc::new(self.registrations.listeners.clone());
            let declarations = Arc::new(declarations);
            // 被替换的 Provider 不执行启动回调。运行期登记也使用同一实例级回调。
            app.provider_booting(move |boot| {
                let registrations = Arc::clone(&registrations);
                let declarations = Arc::clone(&declarations);
                let hooks = hooks.clone();
                Box::pin(async move {
                    let dispatcher = boot.resolve::<Dispatcher>().await?;
                    if !Arc::ptr_eq(&dispatcher.provider_hooks.directory, &hooks.directory) {
                        return Err(configuration_error(
                            "event_dispatcher_application_mismatch",
                            "事件监听器必须登记到当前应用的派发器",
                        ));
                    }
                    #[cfg(feature = "queue")]
                    let mut queue: Option<Arc<crate::queue::Queue>> = None;
                    let mut listeners = Vec::with_capacity(registrations.len());
                    for registration in registrations.iter() {
                        match registration {
                            ListenerRegistration::Direct(listener) => {
                                listeners.push(Arc::clone(listener));
                            }
                            #[cfg(feature = "queue")]
                            ListenerRegistration::Queued { build, .. } => {
                                if queue.is_none() {
                                    if !boot.is_bound::<crate::queue::Queue>() {
                                        return Err(Error::service_initialization::<Dispatcher>(
                                            Error::custom(
                                                "events_require_queue",
                                                "queued listeners require a Queue binding",
                                            ),
                                        ));
                                    }
                                    queue = Some(boot.resolve::<crate::queue::Queue>().await?);
                                }
                                if let Some(queue) = &queue {
                                    listeners.push(build((**queue).clone()));
                                }
                            }
                        }
                    }
                    dispatcher.publish_regular(
                        RegularListeners {
                            position,
                            listeners,
                        },
                        &declarations,
                    )
                })
            });
            Ok(())
        })
    }
}

/// 惰性登记应用唯一的 [`Dispatcher`]，并提供同实例具名键 `events`。
pub struct EventServiceProvider;

impl crate::support::ServiceProvider for EventServiceProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !app.is_bound::<Dispatcher>() {
                let hooks = app.deferred_event_hooks();
                app.singleton::<Dispatcher, _>(move |_| {
                    let hooks = hooks.clone();
                    Box::pin(async { Ok(Arc::new(Dispatcher::new(hooks))) })
                })
                .await?;
            }
            app.ensure_alias(
                ServiceKey::typed::<Dispatcher>(),
                ServiceKey::named("events"),
            )
        })
    }
}

#[cfg(feature = "queue")]
struct QueuedListener<E, J, F> {
    queue: crate::queue::Queue,
    queue_name: String,
    map: Arc<F>,
    marker: PhantomData<fn(&E) -> J>,
}
#[cfg(feature = "queue")]
impl<E, J, F> ErasedListener for QueuedListener<E, J, F>
where
    E: Event,
    J: crate::queue::Job + serde::Serialize,
    F: Fn(&E) -> Result<J> + Send + Sync,
{
    fn selector(&self) -> &str {
        E::NAME
    }
    fn declaration(&self) -> Option<(&'static str, TypeId)> {
        Some((E::NAME, TypeId::of::<E>()))
    }
    fn is_queued(&self) -> bool {
        true
    }
    fn call<'a>(
        &'a self,
        payload: PayloadRef<'a>,
        context: EventContext<'a>,
    ) -> BoxFuture<'a, Result<Invocation>> {
        Box::pin(async move {
            let event = payload.downcast_ref::<E>().ok_or_else(mismatch)?;
            let job = (self.map)(event)?;
            if let Some(transaction) = context.transaction {
                self.queue
                    .dispatch_in_transaction(transaction, &self.queue_name, J::NAME, &job)
                    .await?;
            } else {
                self.queue.dispatch(&self.queue_name, J::NAME, &job).await?;
            }
            Ok(Invocation::Response(DataValue::Null))
        })
    }
}

/// 应用共享的单一事件派发器。克隆共享监听组和类身份约束。
#[derive(Clone)]
pub struct Dispatcher {
    provider_hooks: DeferredEventHooks,
    state: Arc<Mutex<DispatcherState>>,
}

#[derive(Default)]
struct DispatcherState {
    regular: Option<RegularListeners>,
    observed: HashMap<&'static str, TypeId>,
}

struct RegularListeners {
    position: usize,
    listeners: Vec<Listener>,
}

enum DispatchResult {
    Responses(Vec<DataValue>),
    Until(DataValue),
}
impl Dispatcher {
    fn new(provider_hooks: DeferredEventHooks) -> Self {
        Self {
            provider_hooks,
            state: Arc::default(),
        }
    }

    fn state(&self) -> Result<MutexGuard<'_, DispatcherState>> {
        self.state
            .lock()
            .map_err(|_| configuration_error("event_identity_poisoned", "事件身份登记锁已损坏"))
    }

    fn publish_regular(
        &self,
        regular: RegularListeners,
        declarations: &HashMap<&'static str, TypeId>,
    ) -> Result<()> {
        let retired = {
            let mut state = self.state()?;
            // 队列解析可挂起。已经发布的后继登记不接受旧回调回填。
            if state
                .regular
                .as_ref()
                .is_some_and(|current| current.position >= regular.position)
            {
                return Ok(());
            }
            let mut observed = state.observed.clone();
            for (&name, &identity) in declarations {
                observe(&mut observed, name, identity)?;
            }
            state.observed = observed;
            state.regular.replace(regular)
        };
        if retired.is_some_and(|group| dispose_listeners(group.listeners)) {
            return Err(Error::custom(
                "resource_task_interrupted",
                "事件监听组已替换，但旧监听器析构异常",
            ));
        }
        Ok(())
    }

    fn observe<E: Event>(&self) -> Result<()> {
        observe(&mut self.state()?.observed, E::NAME, TypeId::of::<E>())
    }

    fn selected(&self, name: &str, direct_only: bool) -> Result<Vec<Listener>> {
        let state = self.state()?;
        let regular = state.regular.as_ref();
        let snapshot = self.provider_hooks.snapshot(regular);
        let listeners = snapshot
            .as_deref()
            .or_else(|| regular.map(|group| group.listeners.as_slice()))
            .unwrap_or_default();
        let eligible = |listener: &&Listener| !direct_only || !listener.is_queued();
        let mut selected: Vec<_> = listeners
            .iter()
            .filter(eligible)
            .filter(|listener| !listener.selector().contains('*') && listener.selector() == name)
            .cloned()
            .collect();
        let mut patterns = Vec::new();
        for listener in listeners.iter().filter(eligible) {
            let pattern = listener.selector();
            if pattern.contains('*') && !patterns.contains(&pattern) {
                patterns.push(pattern);
            }
        }
        for pattern in patterns {
            if wildcard_name_matches(pattern, name) {
                selected.extend(
                    listeners
                        .iter()
                        .filter(eligible)
                        .filter(|listener| listener.selector() == pattern)
                        .cloned(),
                );
            }
        }
        // 快照可能持有已退役监听器的最后一个引用。
        drop(state);
        Ok(selected)
    }

    // 选择完成后不持身份锁；逐项借原对象，错误和传播停止不合并成业务否决。
    async fn invoke(
        &self,
        name: &str,
        event: PayloadRef<'_>,
        transaction: Option<&DatabaseTransaction>,
        halt: bool,
        direct_only: bool,
    ) -> Result<DispatchResult> {
        let mut responses = Vec::new();
        for listener in self.selected(name, direct_only)? {
            match listener
                .call(event, EventContext { transaction, name })
                .await?
            {
                Invocation::Stop => break,
                Invocation::Response(response) => {
                    if halt {
                        if !matches!(response, DataValue::Null) {
                            return Ok(DispatchResult::Until(response));
                        }
                    } else {
                        if matches!(response, DataValue::Bool(false)) {
                            break;
                        }
                        responses.push(response);
                    }
                }
            }
        }
        Ok(if halt {
            DispatchResult::Until(DataValue::Null)
        } else {
            DispatchResult::Responses(responses)
        })
    }
    async fn responses<P: Any + Send + Sync>(
        &self,
        name: &str,
        event: &P,
        transaction: Option<&DatabaseTransaction>,
        direct_only: bool,
    ) -> Result<Option<Vec<DataValue>>> {
        let DispatchResult::Responses(responses) = self
            .invoke(name, PayloadRef(event), transaction, false, direct_only)
            .await?
        else {
            return Err(configuration_error(
                "event_response_mode_invalid",
                "事件响应模式不匹配",
            ));
        };
        Ok(Some(responses))
    }
    async fn first<P: Any + Send + Sync>(
        &self,
        name: &str,
        event: &P,
        transaction: Option<&DatabaseTransaction>,
    ) -> Result<DataValue> {
        let DispatchResult::Until(value) = self
            .invoke(name, PayloadRef(event), transaction, true, false)
            .await?
        else {
            return Err(configuration_error(
                "event_response_mode_invalid",
                "事件响应模式不匹配",
            ));
        };
        Ok(value)
    }
    /// 类事件普通派发；立即结果总是 `Some`，无人监听为 `Some([])`。
    ///
    /// 当前没有延迟/后提交接口，未来真实延迟才可能返回 `None`。普通 false 停止且不收集。
    pub async fn dispatch<E: Event>(&self, event: &E) -> Result<Option<Vec<DataValue>>> {
        self.observe::<E>()?;
        self.responses(E::NAME, event, None, false).await
    }
    /// 按调用者给出的完整名派发，不以载荷类型预筛选或登记类名。
    pub async fn dispatch_named<P: Any + Send + Sync>(
        &self,
        name: &str,
        event: &P,
    ) -> Result<Option<Vec<DataValue>>> {
        self.responses(name, event, None, false).await
    }
    /// 类事件首个非 null 原值；无响应为 null，false 也是结果。
    pub async fn until<E: Event>(&self, event: &E) -> Result<DataValue> {
        self.observe::<E>()?;
        self.first(E::NAME, event, None).await
    }
    /// 字符串事件首个非 null 原值。
    pub async fn until_named<P: Any + Send + Sync>(
        &self,
        name: &str,
        event: &P,
    ) -> Result<DataValue> {
        self.first(name, event, None).await
    }
    /// 以调用者事务派发类事件；错误由调用者传播以回滚，派发器不提交事务。
    pub async fn dispatch_in_transaction<E: Event>(
        &self,
        transaction: &DatabaseTransaction,
        event: &E,
    ) -> Result<Option<Vec<DataValue>>> {
        self.observe::<E>()?;
        self.responses(E::NAME, event, Some(transaction), false)
            .await
    }
    /// 同一选择/执行循环的具名事务派发。
    pub async fn dispatch_named_in_transaction<P: Any + Send + Sync>(
        &self,
        transaction: &DatabaseTransaction,
        name: &str,
        event: &P,
    ) -> Result<Option<Vec<DataValue>>> {
        self.responses(name, event, Some(transaction), false).await
    }
    /// 在调用者事务中取得类事件首个非 null 响应。
    pub async fn until_in_transaction<E: Event>(
        &self,
        transaction: &DatabaseTransaction,
        event: &E,
    ) -> Result<DataValue> {
        self.observe::<E>()?;
        self.first(E::NAME, event, Some(transaction)).await
    }
    /// 在调用者事务中取得字符串事件首个非 null 响应。
    pub async fn until_named_in_transaction<P: Any + Send + Sync>(
        &self,
        transaction: &DatabaseTransaction,
        name: &str,
        event: &P,
    ) -> Result<DataValue> {
        self.first(name, event, Some(transaction)).await
    }
    /// 仅直接监听器；成功响应显式丢弃，用于提交后尽力通知，不重新排队。
    pub async fn dispatch_direct<E: Event>(&self, event: &E) -> Result<()> {
        self.observe::<E>()?;
        self.responses(E::NAME, event, None, true).await?;
        Ok(())
    }
    #[cfg(feature = "auth")]
    pub(crate) async fn dispatch_direct_in_transaction<E: Event>(
        &self,
        transaction: &DatabaseTransaction,
        event: &E,
    ) -> Result<()> {
        self.observe::<E>()?;
        self.responses(E::NAME, event, Some(transaction), true)
            .await?;
        Ok(())
    }
}
#[cfg(test)]
#[path = "../tests/unit/events/dispatch.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/events/provider.rs"]
mod provider_tests;

#[cfg(all(test, feature = "queue"))]
#[path = "../tests/unit/events/queued.rs"]
mod queued_tests;

#[cfg(test)]
#[path = "../tests/unit/events/responses.rs"]
mod response_tests;

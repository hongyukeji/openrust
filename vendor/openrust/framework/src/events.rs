//! 显式类身份、统一具名选择器及响应的进程内事件。
//!
//! exact 组先执行，随后按 wildcard 模式首次登记次序执行各组。类和同名字符串
//! 使用相同选择器；载荷只在到达类型化回调时检查。队列映射在其监听器位置执行。
//! 当前只支持单参数载荷和立即响应；接口投影、延迟及后提交派发仍未实现。

use crate::{
    core::{Error, Result},
    database::DatabaseTransaction,
    support::{BoxFuture, DataValue},
};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Mutex},
};

/// 类型所有者声明的稳定事件类身份；不是 Rust 诊断类型名。
///
/// 同一派发器已观察到的类名只能对应一个实际类型。名字不可为空或含 `*`。
/// 字符串派发不声明类身份，也不受这两个类名限制。
///
/// ```
/// # async fn example() -> openrust::Result<()> {
/// use openrust::{events::{Event, Events, EventsServiceProvider}, support::DataValue};
/// struct Changed;
/// impl Event for Changed { const NAME: &'static str = r"App\Events\Changed"; }
/// let mut provider = EventsServiceProvider::new();
/// provider.listen_reply::<Changed, _>(|_, context| {
///     assert_eq!(context.event_name(), Changed::NAME);
///     Box::pin(async { Ok(DataValue::Bool(true)) })
/// });
/// let mut builder = openrust::foundation::ProviderBuilder::new();
/// builder.provider(provider);
/// let runtime = builder.start(figment::Figment::new()).await?;
/// let events = runtime.container().resolve::<Events>().await?;
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
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> crate::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let registrations = self.registrations.listeners.clone();
            let mut declarations = HashMap::new();
            for registration in &registrations {
                if let Some((name, identity)) = registration.declaration() {
                    observe(&mut declarations, name, identity)?;
                }
            }
            app.singleton::<Events, _>(move |resolver| {
                #[cfg(not(feature = "queue"))]
                let _ = resolver;
                let registrations = registrations.clone();
                Box::pin(async move {
                    #[cfg(feature = "queue")]
                    let mut queue: Option<Arc<crate::queue::Queue>> = None;
                    let mut listeners = Vec::with_capacity(registrations.len());
                    for registration in &registrations {
                        match registration {
                            ListenerRegistration::Direct(listener) => {
                                listeners.push(Arc::clone(listener));
                            }
                            #[cfg(feature = "queue")]
                            ListenerRegistration::Queued { build, .. } => {
                                if queue.is_none() {
                                    if !resolver.is_bound::<crate::queue::Queue>()? {
                                        return Err(Error::custom(
                                            "events_require_queue",
                                            "queued listeners require a Queue binding",
                                        ));
                                    }
                                    queue = Some(resolver.resolve::<crate::queue::Queue>().await?);
                                }
                                if let Some(queue) = &queue {
                                    listeners.push(build((**queue).clone()));
                                }
                            }
                        }
                    }
                    Ok(Arc::new(Events::new(listeners)?))
                })
            })
            .await?;
            app.require::<Events>();
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut crate::foundation::BootContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let _ = app.resolve::<Events>().await?;
            Ok(())
        })
    }
}

/// 仅补空事件服务；独立类型身份避免抢占随后嵌套登记的真实监听 Provider。
/// 仅在尚无事件服务时登记默认总线，不占用真实事件 Provider 的身份。
#[doc(hidden)]
pub struct DefaultEventsServiceProvider;

impl crate::support::ServiceProvider for DefaultEventsServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut crate::foundation::RegisterContext<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !app.is_bound::<Events>() {
                app.singleton::<Events, _>(|_| {
                    Box::pin(async { Ok(Arc::new(Events::new(Vec::new())?)) })
                })
                .await?;
                app.require::<Events>();
            }
            Ok(())
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

/// 应用共享的单一事件派发器；clone 共享已观察的类身份约束。
#[derive(Clone)]
pub struct Events {
    listeners: Vec<Arc<dyn ErasedListener>>,
    observed: Arc<Mutex<HashMap<&'static str, TypeId>>>,
}
enum DispatchResult {
    Responses(Vec<DataValue>),
    Until(DataValue),
}
impl Events {
    fn new(listeners: Vec<Arc<dyn ErasedListener>>) -> Result<Self> {
        let mut observed = HashMap::new();
        for listener in &listeners {
            if let Some((name, identity)) = listener.declaration() {
                observe(&mut observed, name, identity)?;
            }
        }
        Ok(Self {
            listeners,
            observed: Arc::new(Mutex::new(observed)),
        })
    }
    fn observe<E: Event>(&self) -> Result<()> {
        let mut names = self
            .observed
            .lock()
            .map_err(|_| configuration_error("event_identity_poisoned", "事件身份登记锁已损坏"))?;
        observe(&mut names, E::NAME, TypeId::of::<E>())
    }
    fn selected(&self, name: &str, direct_only: bool) -> Vec<&dyn ErasedListener> {
        let eligible = |listener: &&Arc<dyn ErasedListener>| !direct_only || !listener.is_queued();
        let mut selected: Vec<&dyn ErasedListener> = self
            .listeners
            .iter()
            .filter(eligible)
            .filter(|listener| !listener.selector().contains('*') && listener.selector() == name)
            .map(AsRef::as_ref)
            .collect();
        let mut patterns = Vec::new();
        for listener in self.listeners.iter().filter(eligible) {
            let pattern = listener.selector();
            if pattern.contains('*') && !patterns.contains(&pattern) {
                patterns.push(pattern);
            }
        }
        for pattern in patterns {
            if wildcard_name_matches(pattern, name) {
                selected.extend(
                    self.listeners
                        .iter()
                        .filter(eligible)
                        .filter(|listener| listener.selector() == pattern)
                        .map(AsRef::as_ref),
                );
            }
        }
        selected
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
        for listener in self.selected(name, direct_only) {
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
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{Events, EventsServiceProvider};
    use crate::foundation::ProviderBuilder;
    use figment::Figment;

    #[derive(Debug)]
    struct Probe(u8);
    impl super::Event for Probe {
        const NAME: &'static str = "Tests\\Events\\Probe";
    }

    async fn events(provider: EventsServiceProvider) -> crate::Result<Events> {
        let mut builder = ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(Figment::new()).await?;
        Ok((*runtime.container().resolve::<Events>().await?).clone())
    }

    #[tokio::test]
    async fn listeners_run_in_order_and_stop_after_the_first_error() -> crate::Result<()> {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut plugin = EventsServiceProvider::new();
        let first = Arc::clone(&calls);
        plugin.listen::<Probe, _>(move |event, _| {
            let calls = Arc::clone(&first);
            Box::pin(async move {
                calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event.0);
                Err(crate::Error::custom(
                    "rejected",
                    "probe listener rejected the event",
                ))
            })
        });
        let second = Arc::clone(&calls);
        plugin.listen::<Probe, _>(move |event, _| {
            let calls = Arc::clone(&second);
            Box::pin(async move {
                calls
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(event.0 + 1);
                Ok(())
            })
        });
        let dispatcher = events(plugin).await?;
        assert!(dispatcher.dispatch(&Probe(4)).await.is_err());
        assert_eq!(
            calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_slice(),
            [4]
        );
        Ok(())
    }

    #[tokio::test]
    async fn unrelated_event_types_are_not_delivered() -> crate::Result<()> {
        #[derive(Debug)]
        struct Other;
        impl super::Event for Other {
            const NAME: &'static str = "Tests\\Events\\Other";
        }
        let calls = Arc::new(Mutex::new(0));
        let observed = Arc::clone(&calls);
        let mut plugin = EventsServiceProvider::new();
        plugin.listen::<Probe, _>(move |_, _| {
            let observed = Arc::clone(&observed);
            Box::pin(async move {
                *observed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
                Ok(())
            })
        });
        events(plugin).await?.dispatch(&Other).await?;
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            0
        );
        Ok(())
    }
    #[test]
    fn named_patterns_match_full_case_sensitive_names_and_only_star_is_special() {
        for (pattern, name, expected) in [
            ("modules.*.enabled", "modules.blog.enabled", true),
            ("modules.*.enabled", "modules.a.b.enabled", true),
            ("modules.*.enabled", "prefix.modules.blog.enabled", false),
            ("Modules.*", "modules.blog.enabled", false),
            ("modules.?.enabled", "modules.a.enabled", false),
            ("modules.?.enabled", "modules.?.enabled", true),
            ("*", "", true),
            ("a**b", "ab", true),
            ("é*", "éß", true),
            ("a*b", "ac", false),
            ("", "x", false),
        ] {
            assert_eq!(
                super::wildcard_name_matches(pattern, name),
                expected,
                "{pattern:?} / {name:?}"
            );
        }
    }

    #[tokio::test]
    async fn canonical_and_string_share_identity_but_other_names_do_not() -> crate::Result<()> {
        use super::{Event, EventPropagation};
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut provider = EventsServiceProvider::new();
        let typed = Arc::clone(&calls);
        provider.listen::<Probe, _>(move |event, context| {
            let typed = Arc::clone(&typed);
            Box::pin(async move {
                typed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(("typed", event.0, context.event_name().to_owned()));
                Ok(())
            })
        });
        let named = Arc::clone(&calls);
        provider.listen_named::<Probe, _>("modules.*.enabled", move |event, context| {
            let named = Arc::clone(&named);
            Box::pin(async move {
                named
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(("named", event.0, context.event_name().to_owned()));
                Ok(EventPropagation::Continue)
            })
        });
        let events = events(provider).await?;
        events.dispatch(&Probe(1)).await?;
        events
            .dispatch_named("modules.blog.disabled", &Probe(2))
            .await?;
        events
            .dispatch_named("modules.blog.enabled", &Probe(3))
            .await?;
        events.dispatch_named(Probe::NAME, &Probe(4)).await?;
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            [
                ("typed", 1, Probe::NAME.to_owned()),
                ("named", 3, "modules.blog.enabled".to_owned()),
                ("typed", 4, Probe::NAME.to_owned())
            ]
        );
        Ok(())
    }
}

#[cfg(test)]
mod provider_tests {
    struct SingletonProbe;
    impl super::Event for SingletonProbe {
        const NAME: &'static str = "Tests\\SingletonProbe";
    }
    use super::*;
    use std::sync::Mutex;

    #[tokio::test]
    async fn provider_shares_dispatcher_and_preserves_order_and_short_circuit() -> Result<()> {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut provider = EventsServiceProvider::new();
        for index in 0..3 {
            let calls = Arc::clone(&calls);
            provider.listen::<SingletonProbe, _>(move |_, _| {
                let calls = Arc::clone(&calls);
                Box::pin(async move {
                    calls
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(index);
                    if index == 1 {
                        Err(Error::custom("listener_rejected", "rejected"))
                    } else {
                        Ok(())
                    }
                })
            });
        }
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder.provider(provider);
        let runtime = builder.start(figment::Figment::new()).await?;
        let first = runtime.container().resolve::<Events>().await?;
        let second = runtime.container().resolve::<Events>().await?;
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(
            first
                .dispatch(&SingletonProbe)
                .await
                .err()
                .map(|error| error.code().to_owned())
                .as_deref(),
            Some("listener_rejected")
        );
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            vec![0, 1]
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }
}

#[cfg(all(test, feature = "queue"))]
mod queued_tests {
    use std::sync::{Arc, Mutex};

    use crate::Result;
    use crate::config::QueueConfig;
    use crate::database::DatabaseTransaction;
    use crate::events::Events;
    use crate::events::EventsServiceProvider;
    use crate::foundation::ProviderBuilder;
    use crate::foundation::RegisterContext;
    use crate::queue::DispatchOptions;
    use crate::queue::Job;
    use crate::queue::JobContext;
    use crate::queue::Queue;
    use crate::queue::QueueDriver;
    use crate::support::{BoxFuture, ServiceProvider};
    use serde_json::Value;

    struct QueueBinding(Queue);
    impl ServiceProvider for QueueBinding {
        fn register<'a>(
            &'a self,
            app: &'a mut RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::new(self.0.clone())).await?;
                Ok(())
            })
        }
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct UserRegistered {
        user_id: i64,
    }

    impl super::Event for UserRegistered {
        const NAME: &'static str = "Tests\\UserRegistered";
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct WelcomeJob {
        account_id: i64,
    }

    impl Job for WelcomeJob {
        const NAME: &'static str = "tests.events.welcome.v1";

        fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[derive(Clone, Default)]
    struct CaptureQueueDriver {
        dispatched: Arc<Mutex<Vec<(String, String, Value)>>>,
        fail_dispatch: bool,
    }

    impl QueueDriver for CaptureQueueDriver {
        fn dispatch<'a>(
            &'a self,
            queue: &'a str,
            job_name: &'a str,
            payload: Value,
            _options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                if self.fail_dispatch {
                    return Err(crate::Error::custom(
                        "queue_unavailable",
                        "queue store rejected dispatch",
                    ));
                }
                self.dispatched
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((queue.to_owned(), job_name.to_owned(), payload));
                Ok("captured-job".to_owned())
            })
        }

        fn dispatch_in_transaction<'a>(
            &'a self,
            _transaction: &'a DatabaseTransaction,
            queue: &'a str,
            job_name: &'a str,
            payload: Value,
            options: DispatchOptions,
        ) -> BoxFuture<'a, Result<String>> {
            self.dispatch(queue, job_name, payload, options)
        }

        fn run_next<'a>(&'a self, _queue: &'a str) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async { Ok(false) })
        }

        fn retry_failed<'a>(&'a self, _failed_id: &'a str) -> BoxFuture<'a, Result<String>> {
            Box::pin(async { Ok("captured-retry".to_owned()) })
        }

        fn reset_shutdown(&self) {}

        fn request_shutdown(&self) {}

        fn is_shutdown_requested(&self) -> bool {
            false
        }
    }

    struct LateQueue(Queue);
    impl crate::support::ServiceProvider for LateQueue {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.instance(Arc::new(self.0.clone())).await?;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn provider_resolves_queue_registered_later_and_fails_when_missing() -> Result<()> {
        fn events_provider() -> super::EventsServiceProvider {
            let mut provider = super::EventsServiceProvider::new();
            provider.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
                Ok(WelcomeJob {
                    account_id: event.user_id,
                })
            });
            provider
        }
        let mut missing = crate::foundation::ProviderBuilder::new();
        missing.provider(events_provider());
        let error = missing
            .start(figment::Figment::new())
            .await
            .err()
            .ok_or_else(|| crate::Error::custom("test", "missing error"))?;
        assert_eq!(error.code(), "service_initialization_failed");
        match &error {
            crate::Error::ServiceInitialization { source, .. } => {
                assert_eq!(source.code(), "events_require_queue");
            }
            other => {
                return Err(crate::Error::custom(
                    "test",
                    format!("unexpected: {other:?}"),
                ));
            }
        }
        let driver = CaptureQueueDriver::default();
        let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder
            .provider(events_provider())
            .provider(LateQueue(queue));
        let runtime = builder.start(figment::Figment::new()).await?;
        runtime
            .container()
            .resolve::<Events>()
            .await?
            .dispatch(&UserRegistered { user_id: 73 })
            .await?;
        assert_eq!(
            driver
                .dispatched
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
            1
        );
        runtime
            .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            .await
    }

    struct BrokenQueue;
    impl crate::support::ServiceProvider for BrokenQueue {
        fn register<'a>(
            &'a self,
            app: &'a mut crate::foundation::RegisterContext<'_>,
        ) -> crate::support::BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                app.singleton::<Queue, _>(|resolver| {
                    Box::pin(async move {
                        let _ = resolver.resolve::<u128>().await?;
                        Err(crate::Error::custom("test", "unreachable"))
                    })
                })
                .await?;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn bound_queue_internal_missing_dependency_is_not_misreported() -> Result<()> {
        let mut events = super::EventsServiceProvider::new();
        events.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
            Ok(WelcomeJob {
                account_id: event.user_id,
            })
        });
        let mut builder = crate::foundation::ProviderBuilder::new();
        builder.provider(events).provider(BrokenQueue);
        let error = builder
            .start(figment::Figment::new())
            .await
            .err()
            .ok_or_else(|| crate::Error::custom("test", "missing error"))?;
        assert_eq!(error.code(), "service_not_bound");
        assert!(!error.to_string().contains("queued listeners require"));
        Ok(())
    }

    #[tokio::test]
    async fn queued_listener_maps_event_to_registered_job_payload() -> Result<()> {
        let driver = CaptureQueueDriver::default();
        let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
        let mut builder = ProviderBuilder::new();
        builder.provider(QueueBinding(queue));

        let mut plugin = EventsServiceProvider::new();
        plugin.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
            Ok(WelcomeJob {
                account_id: event.user_id,
            })
        });
        builder.provider(plugin);
        let app = builder.start(figment::Figment::new()).await?;
        let events = app.container().resolve::<Events>().await?;
        events
            .dispatch_direct(&UserRegistered { user_id: 99 })
            .await?;
        events
            .dispatch_named(
                <UserRegistered as super::Event>::NAME,
                &UserRegistered { user_id: 73 },
            )
            .await?;

        let dispatched = driver
            .dispatched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].0, "mail");
        assert_eq!(dispatched[0].1, WelcomeJob::NAME);
        assert_eq!(dispatched[0].2, serde_json::json!({"account_id": 73}));
        Ok(())
    }

    #[tokio::test]
    async fn queued_listener_mapping_error_stops_later_listeners() -> Result<()> {
        let driver = CaptureQueueDriver::default();
        let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
        let mut builder = ProviderBuilder::new();
        builder.provider(QueueBinding(queue));

        let later_listener_called = Arc::new(Mutex::new(false));
        let mut plugin = EventsServiceProvider::new();
        plugin.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |_| {
            Err(crate::Error::custom(
                "mapping_rejected",
                "event cannot be mapped to a welcome job",
            ))
        });
        let called = Arc::clone(&later_listener_called);
        plugin.listen::<UserRegistered, _>(move |_, _| {
            let called = Arc::clone(&called);
            Box::pin(async move {
                *called
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                Ok(())
            })
        });
        builder.provider(plugin);
        let app = builder.start(figment::Figment::new()).await?;
        let events = app.container().resolve::<Events>().await?;

        assert!(
            events
                .dispatch(&UserRegistered { user_id: 73 })
                .await
                .is_err()
        );
        assert!(
            driver
                .dispatched
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        assert!(
            !*later_listener_called
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        );
        Ok(())
    }

    #[tokio::test]
    async fn queued_dispatch_error_stops_later_listeners() -> Result<()> {
        let driver = CaptureQueueDriver {
            fail_dispatch: true,
            ..CaptureQueueDriver::default()
        };
        let queue = Queue::with_driver(driver.clone(), &QueueConfig::default())?;
        let mut builder = ProviderBuilder::new();
        builder.provider(QueueBinding(queue));

        let later_listener_called = Arc::new(Mutex::new(false));
        let mut plugin = EventsServiceProvider::new();
        plugin.listen_queued::<UserRegistered, WelcomeJob, _>("mail", |event| {
            Ok(WelcomeJob {
                account_id: event.user_id,
            })
        });
        let called = Arc::clone(&later_listener_called);
        plugin.listen::<UserRegistered, _>(move |_, _| {
            let called = Arc::clone(&called);
            Box::pin(async move {
                *called
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                Ok(())
            })
        });
        builder.provider(plugin);
        let app = builder.start(figment::Figment::new()).await?;
        let events = app.container().resolve::<Events>().await?;

        assert!(
            events
                .dispatch(&UserRegistered { user_id: 73 })
                .await
                .is_err()
        );
        assert!(
            driver
                .dispatched
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        assert!(
            !*later_listener_called
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        );
        Ok(())
    }
}

#[cfg(test)]
#[path = "events/response_tests.rs"]
mod response_tests;

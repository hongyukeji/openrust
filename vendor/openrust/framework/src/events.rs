//! 类型化、有序的进程内事件，以及显式持久化的队列监听器。
//!
//! 进程内监听器按注册顺序运行，并在遇到首个错误时停止。事件不会持久化，也不会跨进程广播。
//! 若派发必须在进程退出后继续，应在业务事务中使用队列监听器。标准数据库服务
//! 会在 `Db::transaction` 成功提交前派发事务校验事件，并在提交后尽力通知 direct listeners；
//! 可靠副作用仍须在事务内排队。

use std::{
    any::{Any, TypeId},
    marker::PhantomData,
    sync::Arc,
};

use crate::core::Error;
use crate::core::Result;
use crate::database::DatabaseTransaction;
use crate::support::BoxFuture;

/// 事件监听器可访问的上下文。
#[derive(Clone, Copy)]
pub struct EventContext<'a> {
    transaction: Option<&'a DatabaseTransaction>,
    name: Option<&'a str>,
}

impl<'a> EventContext<'a> {
    /// 通过 [`Events::dispatch_in_transaction`] 派发时，返回调用方的事务。
    pub const fn transaction(&self) -> Option<&'a DatabaseTransaction> {
        self.transaction
    }

    /// 命名派发时的完整事件名称；普通类型派发返回 `None`。
    pub const fn event_name(&self) -> Option<&'a str> {
        self.name
    }
}

/// 命名监听器对当前传播的控制；停止传播不等于取消触发该事件的业务操作。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventPropagation {
    /// 继续调用后续匹配的监听器。
    Continue,
    /// 当前事件不再调用后续监听器，派发仍成功返回。
    Stop,
}

trait ErasedListener: Send + Sync {
    fn event_type(&self) -> TypeId;

    fn accepts_name(&self, _name: Option<&str>) -> bool {
        true
    }

    fn is_queued(&self) -> bool {
        false
    }

    fn call<'a>(
        &'a self,
        event: &'a dyn Any,
        transaction: Option<&'a DatabaseTransaction>,
        name: Option<&'a str>,
    ) -> BoxFuture<'a, Result<()>>;

    fn call_named<'a>(
        &'a self,
        event: &'a dyn Any,
        transaction: Option<&'a DatabaseTransaction>,
        name: &'a str,
    ) -> BoxFuture<'a, Result<EventPropagation>> {
        let pending = self.call(event, transaction, Some(name));
        Box::pin(async move {
            pending.await?;
            Ok(EventPropagation::Continue)
        })
    }
}

struct TypedListener<E, F> {
    handler: F,
    marker: PhantomData<fn(&E)>,
}

impl<E, F> ErasedListener for TypedListener<E, F>
where
    E: Any + Send + Sync + 'static,
    F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<()>> + Send + Sync,
{
    fn event_type(&self) -> TypeId {
        TypeId::of::<E>()
    }

    fn call<'a>(
        &'a self,
        event: &'a dyn Any,
        transaction: Option<&'a DatabaseTransaction>,
        name: Option<&'a str>,
    ) -> BoxFuture<'a, Result<()>> {
        let Some(event) = event.downcast_ref::<E>() else {
            return Box::pin(async {
                Err(Error::custom(
                    "event_type_mismatch",
                    "event listener received an incompatible event type",
                ))
            });
        };
        (self.handler)(event, EventContext { transaction, name })
    }
}

struct NamedListener<E, F> {
    pattern: String,
    handler: F,
    marker: PhantomData<fn(&E)>,
}

impl<E, F> ErasedListener for NamedListener<E, F>
where
    E: Any + Send + Sync + 'static,
    F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<EventPropagation>> + Send + Sync,
{
    fn event_type(&self) -> TypeId {
        TypeId::of::<E>()
    }

    fn accepts_name(&self, name: Option<&str>) -> bool {
        name.is_some_and(|name| wildcard_name_matches(&self.pattern, name))
    }

    fn call<'a>(
        &'a self,
        event: &'a dyn Any,
        transaction: Option<&'a DatabaseTransaction>,
        name: Option<&'a str>,
    ) -> BoxFuture<'a, Result<()>> {
        let pending = self.call_named(event, transaction, name.unwrap_or_default());
        Box::pin(async move { pending.await.map(|_| ()) })
    }

    fn call_named<'a>(
        &'a self,
        event: &'a dyn Any,
        transaction: Option<&'a DatabaseTransaction>,
        name: &'a str,
    ) -> BoxFuture<'a, Result<EventPropagation>> {
        let Some(event) = event.downcast_ref::<E>() else {
            return Box::pin(async {
                Err(Error::custom(
                    "event_type_mismatch",
                    "事件监听器的载荷类型不匹配",
                ))
            });
        };
        (self.handler)(
            event,
            EventContext {
                transaction,
                name: Some(name),
            },
        )
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
        build: Arc<dyn Fn(crate::queue::Queue) -> Arc<dyn ErasedListener> + Send + Sync>,
    },
}

/// 按确定的顺序注册应用事件监听器。
#[derive(Default)]
struct ListenerRegistrations {
    listeners: Vec<ListenerRegistration>,
}

impl ListenerRegistrations {
    fn listen_named<E, F>(&mut self, pattern: impl Into<String>, handler: F) -> &mut Self
    where
        E: Any + Send + Sync + 'static,
        F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<EventPropagation>>
            + Send
            + Sync
            + 'static,
    {
        self.listeners
            .push(ListenerRegistration::Direct(Arc::new(NamedListener::<
                E,
                F,
            > {
                pattern: pattern.into(),
                handler,
                marker: PhantomData,
            })));
        self
    }

    /// 为一种具体事件类型注册异步监听器。
    pub fn listen<E, F>(&mut self, handler: F) -> &mut Self
    where
        E: Any + Send + Sync + 'static,
        F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<()>> + Send + Sync + 'static,
    {
        self.listeners
            .push(ListenerRegistration::Direct(Arc::new(TypedListener::<
                E,
                F,
            > {
                handler,
                marker: PhantomData,
            })));
        self
    }

    /// 注册持久化监听器，将事件映射为已注册的队列任务。
    ///
    /// 须注册队列服务，且显式启动的 worker 会至少执行一次所生成的任务。
    /// 在事务中派发时，任务会使用同一个事务插入；若事务回滚，任务也会被撤销。
    #[cfg(feature = "queue")]
    pub fn listen_queued<E, J, F>(&mut self, queue_name: impl Into<String>, map: F) -> &mut Self
    where
        E: Any + Send + Sync + 'static,
        J: crate::queue::Job + serde::Serialize,
        F: Fn(&E) -> Result<J> + Send + Sync + 'static,
    {
        let queue_name = queue_name.into();
        let map = Arc::new(map);
        self.listeners.push(ListenerRegistration::Queued {
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

/// 注册事件监听器并通过统一容器提供共享派发器。
///
#[derive(Default)]
pub struct EventsServiceProvider {
    registrations: ListenerRegistrations,
}

impl EventsServiceProvider {
    /// 创建空的事件 Provider。
    pub fn new() -> Self {
        Self::default()
    }

    /// 在同一有序事件链登记命名监听器；`*` 匹配任意片段，名称区分大小写。
    ///
    /// 返回 `Stop` 只停止当前传播；错误向派发调用者传播。普通类型派发不调用此监听器。
    pub fn listen_named<E, F>(&mut self, pattern: impl Into<String>, handler: F) -> &mut Self
    where
        E: Any + Send + Sync + 'static,
        F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<EventPropagation>>
            + Send
            + Sync
            + 'static,
    {
        self.registrations.listen_named::<E, F>(pattern, handler);
        self
    }

    /// 按调用顺序注册直接监听器。
    pub fn listen<E, F>(&mut self, handler: F) -> &mut Self
    where
        E: Any + Send + Sync + 'static,
        F: for<'a> Fn(&'a E, EventContext<'a>) -> BoxFuture<'a, Result<()>> + Send + Sync + 'static,
    {
        self.registrations.listen::<E, F>(handler);
        self
    }

    /// 注册队列监听器；队列服务从同一容器异步解析。
    #[cfg(feature = "queue")]
    pub fn listen_queued<E, J, F>(&mut self, queue_name: impl Into<String>, map: F) -> &mut Self
    where
        E: Any + Send + Sync + 'static,
        J: crate::queue::Job + serde::Serialize,
        F: Fn(&E) -> Result<J> + Send + Sync + 'static,
    {
        self.registrations.listen_queued::<E, J, F>(queue_name, map);
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
                            ListenerRegistration::Queued { build } => {
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
                    Ok(Arc::new(Events { listeners }))
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
                    Box::pin(async {
                        Ok(Arc::new(Events {
                            listeners: Vec::new(),
                        }))
                    })
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
    E: Any + Send + Sync + 'static,
    J: crate::queue::Job + serde::Serialize,
    F: Fn(&E) -> Result<J> + Send + Sync,
{
    fn is_queued(&self) -> bool {
        true
    }

    fn event_type(&self) -> TypeId {
        TypeId::of::<E>()
    }

    fn call<'a>(
        &'a self,
        event: &'a dyn Any,
        transaction: Option<&'a DatabaseTransaction>,
        _name: Option<&'a str>,
    ) -> BoxFuture<'a, Result<()>> {
        let Some(event) = event.downcast_ref::<E>() else {
            return Box::pin(async {
                Err(Error::custom(
                    "event_type_mismatch",
                    "event listener received an incompatible event type",
                ))
            });
        };
        let job = match (self.map)(event) {
            Ok(job) => job,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        Box::pin(async move {
            if let Some(transaction) = transaction {
                self.queue
                    .dispatch_in_transaction(transaction, &self.queue_name, J::NAME, &job)
                    .await?;
            } else {
                self.queue.dispatch(&self.queue_name, J::NAME, &job).await?;
            }
            Ok(())
        })
    }
}

/// 应用作用域内的类型化事件派发器。
#[derive(Clone)]
pub struct Events {
    listeners: Vec<Arc<dyn ErasedListener>>,
}

impl Events {
    /// 按完整名称和载荷类型派发；类型监听器与匹配的命名监听器共用原注册顺序。
    ///
    /// `Stop` 不取消业务动作；首个错误停止派发并传播给调用方。此方法不跨进程广播。
    pub async fn dispatch_named<E>(&self, name: &str, event: &E) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        for listener in &self.listeners {
            if listener.event_type() == TypeId::of::<E>()
                && listener.accepts_name(Some(name))
                && listener.call_named(event, None, name).await? == EventPropagation::Stop
            {
                break;
            }
        }
        Ok(())
    }
    /// 按注册顺序将事件发送给匹配的监听器。
    ///
    /// 首个监听器错误会停止派发并向调用方传播。此操作仅在当前进程中执行，不会持久化，
    /// 也不会在应用进程间广播。
    pub async fn dispatch<E>(&self, event: &E) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        self.dispatch_with_transaction(event, None).await
    }

    /// 派发事件，并在监听器上下文中提供调用方当前事务。
    ///
    /// 应将此方法的错误传播到事务回调之外，以回滚业务写入和队列监听任务。此方法不会提交事务。
    /// 直接监听器不得在此执行不可逆的外部副作用；请改用 `listen_queued`。
    pub async fn dispatch_in_transaction<E>(
        &self,
        transaction: &DatabaseTransaction,
        event: &E,
    ) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        self.dispatch_with_transaction(event, Some(transaction))
            .await
    }

    /// 仅运行直接监听器；用于已提交事务后的尽力通知，不会再次派发队列任务。
    pub(crate) async fn dispatch_direct<E>(&self, event: &E) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        self.dispatch_direct_with_transaction(event, None).await
    }

    #[cfg(feature = "auth")]
    pub(crate) async fn dispatch_direct_in_transaction<E>(
        &self,
        transaction: &DatabaseTransaction,
        event: &E,
    ) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        self.dispatch_direct_with_transaction(event, Some(transaction))
            .await
    }

    async fn dispatch_direct_with_transaction<E>(
        &self,
        event: &E,
        transaction: Option<&DatabaseTransaction>,
    ) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        for listener in &self.listeners {
            if listener.event_type() == TypeId::of::<E>()
                && listener.accepts_name(None)
                && !listener.is_queued()
            {
                listener.call(event, transaction, None).await?;
            }
        }
        Ok(())
    }

    async fn dispatch_with_transaction<E>(
        &self,
        event: &E,
        transaction: Option<&DatabaseTransaction>,
    ) -> Result<()>
    where
        E: Any + Send + Sync + 'static,
    {
        for listener in &self.listeners {
            if listener.event_type() == TypeId::of::<E>() && listener.accepts_name(None) {
                listener.call(event, transaction, None).await?;
            }
        }
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

    // 防止命名监听泄入普通派发，或为新名字建立另一条丢失旧类型监听器的事件链。
    #[tokio::test]
    async fn typed_and_named_listeners_share_order_and_receive_the_current_name()
    -> crate::Result<()> {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut provider = EventsServiceProvider::new();
        let typed = Arc::clone(&calls);
        provider.listen::<Probe, _>(move |event, context| {
            let typed = Arc::clone(&typed);
            Box::pin(async move {
                typed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(format!("typed:{}:{:?}", event.0, context.event_name()));
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
                    .push(format!("named:{}:{:?}", event.0, context.event_name()));
                Ok(super::EventPropagation::Continue)
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
        assert_eq!(
            *calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            [
                "typed:1:None",
                "typed:2:Some(\"modules.blog.disabled\")",
                "typed:3:Some(\"modules.blog.enabled\")",
                "named:3:Some(\"modules.blog.enabled\")",
            ]
        );
        Ok(())
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use std::sync::Mutex;

    #[tokio::test]
    async fn provider_shares_dispatcher_and_preserves_order_and_short_circuit() -> Result<()> {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut provider = EventsServiceProvider::new();
        for index in 0..3 {
            let calls = Arc::clone(&calls);
            provider.listen::<u8, _>(move |_, _| {
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
                .dispatch(&1u8)
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

    use crate::BoxFuture;
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
    use crate::support::ServiceProvider;
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
        events.dispatch(&UserRegistered { user_id: 73 }).await?;

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

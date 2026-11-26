use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock, Weak},
};

use crate::{
    broadcasting::{BroadcastEvent, BroadcastException, JOB, UniqueBroadcast},
    container::Container,
    contracts::broadcasting::{
        Factory as BroadcastFactory, ShouldBeUnique, ShouldBroadcast, ShouldBroadcastNow,
    },
    contracts::container::{BindingResolutionError, Container as _},
    contracts::events::{
        Dispatcher as Contract, Event, Listener, ShouldDispatchAfterCommit, Subscriber,
    },
    database::{DatabaseError, Transaction},
    support::facades::{application_scope, current_container},
};

use super::listener_registry::{ListenerSource, Registration};

use super::{
    Delivery, DispatchResult, EventContext, EventError, EventFake, EventFuture, EventResult,
    ListenerRegistry, ListenerResponse,
};

#[derive(Default)]
struct Registrations {
    listeners: ListenerRegistry,
    deferred_types: HashSet<TypeId>,
    broadcasts: HashMap<TypeId, BroadcastRegistration>,
    fake: Option<Arc<EventFake>>,
}

type BroadcastUnique = fn(&dyn Event) -> Option<UniqueBroadcast>;

type BroadcastSource = for<'a> fn(&'a dyn Event) -> Option<&'a dyn ShouldBroadcast>;

#[derive(Clone, Copy)]
struct BroadcastRegistration {
    event: BroadcastSource,
    immediate: bool,
    unique: Option<BroadcastUnique>,
}

struct DeferredEvents {
    dispatcher: Arc<RwLock<Registrations>>,
    names: Option<HashSet<String>>,
    events: std::cell::RefCell<Vec<DeferredEvent>>,
}

struct DeferredEvent {
    event: Arc<dyn Event>,
    halt: bool,
    after_commit: bool,
    socket: Option<String>,
}

#[derive(Clone)]
struct ScopedFake {
    dispatcher: Arc<RwLock<Registrations>>,
    fake: Arc<Mutex<Arc<EventFake>>>,
}

tokio::task_local! {
    static DEFERRED_EVENTS: Option<DeferredEvents>;
    static SCOPED_FAKES: Vec<ScopedFake>;
}

#[derive(Default)]
pub(crate) struct PushedEvents(Mutex<PushedState>);

#[derive(Default)]
struct PushedState {
    events: HashMap<String, Vec<Registration>>,
    forgotten_through: Option<u64>,
}

struct PushedEvent {
    name: String,
}
impl Event for PushedEvent {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Registers and dispatches events to listeners in one application.
#[derive(Clone, Default)]
pub struct Dispatcher {
    registrations: Arc<RwLock<Registrations>>,
    owner: Weak<Container>,
    pushed: Arc<PushedEvents>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn for_application(owner: Weak<Container>) -> Self {
        Self {
            owner,
            ..Self::default()
        }
    }

    pub(crate) fn owner(&self) -> Option<Arc<Container>> {
        self.owner.upgrade()
    }

    fn pushed_events(&self) -> EventResult<Arc<PushedEvents>> {
        match self.owner.upgrade() {
            Some(owner) => owner.make::<PushedEvents>().map_err(Into::into),
            None => Ok(self.pushed.clone()),
        }
    }

    pub fn push(&self, event: Arc<dyn Event>) -> EventResult<()> {
        let name = format!("{}_pushed", event.name());
        let sequence = self.write().listeners.next_sequence();
        self.pushed_events()?
            .0
            .lock()
            .expect("事件暂存锁中毒")
            .events
            .entry(name)
            .or_default()
            .push(Registration {
                source: ListenerSource::Pushed(event),
                label: "pushed",
                after_commit: false,
                sequence,
            });
        Ok(())
    }

    pub async fn flush(&self, name: &str, context: &mut EventContext<'_>) -> EventResult<()> {
        self.dispatch(
            Arc::new(PushedEvent {
                name: format!("{name}_pushed"),
            }),
            context,
        )
        .await?;
        Ok(())
    }

    pub fn forget_pushed(&self) -> EventResult<()> {
        let state = self.pushed_events()?;
        let mut state = state.0.lock().expect("事件暂存锁中毒");
        state.events.clear();
        if self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.has_active_scope())
        {
            // 只屏蔽当前执行作用域已经存在的精确监听器，不能删除其他请求的注册。
            state.forgotten_through = Some(self.read().listeners.sequence());
        } else {
            self.write().listeners.forget_pushed();
        }
        Ok(())
    }

    fn queue(&self) -> EventResult<Arc<crate::queue::QueueManager>> {
        self.owner
            .upgrade()
            .ok_or(EventError::QueueUnavailable)?
            .make::<crate::queue::QueueManager>()
            .map_err(Into::into)
    }

    pub fn register(
        &self,
        configure: impl FnOnce(&mut ListenerRegistry) -> EventResult<()>,
    ) -> EventResult<()> {
        let mut registry = ListenerRegistry::default();
        configure(&mut registry)?;
        let handlers = registry.queued_handlers();
        if !handlers.is_empty() {
            self.queue()?
                .register_listeners(handlers)
                .map_err(EventError::Queue)?;
        }
        self.write().listeners.extend(registry);
        Ok(())
    }

    pub fn listen_class<L: Listener + 'static>(
        &self,
        event: &str,
        delivery: Delivery,
    ) -> EventResult<()> {
        self.register(|registry| registry.listen_class::<L>(event, delivery))
    }

    pub fn mark_after_commit<E: ShouldDispatchAfterCommit>(&self) {
        self.write().deferred_types.insert(TypeId::of::<E>());
    }

    /// Registers the broadcast contract for this compiled event type.
    pub fn register_broadcast<E: ShouldBroadcast>(&self) {
        self.register_broadcast_listener::<E>(false, None);
    }

    /// Registers the synchronous broadcast contract for this compiled event type.
    pub fn register_broadcast_now<E: ShouldBroadcastNow>(&self) {
        self.register_broadcast_listener::<E>(true, None);
    }

    /// Registers a broadcast event whose queued deliveries acquire a uniqueness lock.
    pub fn register_unique_broadcast<E: ShouldBeUnique>(&self) {
        self.register_broadcast_listener::<E>(
            false,
            Some(|event| event.downcast_ref::<E>().map(UniqueBroadcast::for_event)),
        );
    }

    fn register_broadcast_listener<E: ShouldBroadcast>(
        &self,
        immediate: bool,
        unique: Option<BroadcastUnique>,
    ) {
        self.write().broadcasts.insert(
            TypeId::of::<E>(),
            BroadcastRegistration {
                event: |event| {
                    event
                        .downcast_ref::<E>()
                        .map(|event| event as &dyn ShouldBroadcast)
                },
                immediate,
                unique,
            },
        );
    }

    pub fn has_listeners(&self, event: &str) -> bool {
        Contract::has_listeners(self, event)
    }

    /// Checks whether the current listeners or event fake can receive the named event.
    pub fn observes(&self, name: &str) -> bool {
        let fake_matches = self
            .active_fake(&self.read())
            .is_some_and(|fake| fake.may_match_name(name));
        self.has_listeners(name) || fake_matches
    }

    pub fn listing(&self) -> Vec<(String, Vec<&'static str>)> {
        let Ok(state) = self.pushed_events() else {
            return self.read().listeners.listing();
        };
        let state = state.0.lock().expect("事件暂存锁中毒");
        let registry = self.read();
        let mut names: Vec<_> = registry
            .listeners
            .listing()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        for name in state.events.keys() {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
            .into_iter()
            .filter_map(|name| {
                // 列表按注册项展示，通配匹配仍在实际派发阶段执行。
                let listeners = registry.listeners.listed_with_pushed(
                    &name,
                    state.events.get(&name).map_or(&[], Vec::as_slice),
                    state.forgotten_through,
                );
                (!listeners.is_empty()).then_some((name, listeners))
            })
            .collect()
    }

    /// Returns matching listeners in delivery order with their actual queue contracts.
    pub fn get_listeners(&self, event: &str) -> Vec<(&'static str, bool)> {
        self.read()
            .listeners
            .selected(event, &[])
            .iter()
            .map(|registration| {
                (
                    registration.label,
                    matches!(registration.source, ListenerSource::Queued(_)),
                )
            })
            .collect()
    }

    /// Whether the compiled event has a registered broadcast contract.
    pub fn is_broadcasting(&self, event: &dyn Event) -> bool {
        self.read().broadcasts.contains_key(&event.type_id())
    }

    /// Delivers selected events in order after the callback succeeds.
    pub async fn defer<T>(
        &self,
        names: Option<&[&str]>,
        callback: impl Future<Output = EventResult<T>>,
    ) -> EventResult<T> {
        let (scope, result) = DEFERRED_EVENTS
            .scope(
                Some(DeferredEvents {
                    dispatcher: self.registrations.clone(),
                    names: names.map(|names| names.iter().map(|name| (*name).into()).collect()),
                    events: Default::default(),
                }),
                async {
                    let result = callback.await;
                    let events = DEFERRED_EVENTS.with(|scope| {
                        std::mem::take(&mut *scope.as_ref().unwrap().events.borrow_mut())
                    });
                    (events, result)
                },
            )
            .await;
        let result = result?;
        DEFERRED_EVENTS
            .scope(None, async {
                for deferred in scope {
                    let mut context = EventContext::new();
                    context.socket = deferred.socket;
                    self.dispatch_event(
                        deferred.event,
                        &mut context,
                        deferred.halt,
                        deferred.after_commit,
                    )
                    .await?;
                }
                Ok(result)
            })
            .await
    }

    pub async fn dispatch_after_commit(
        &self,
        event: impl ShouldDispatchAfterCommit,
        context: &mut EventContext<'_>,
    ) -> EventResult<DispatchResult> {
        self.dispatch_event(Arc::new(event), context, false, true)
            .await
    }

    /// Records the selected events for this application. An empty slice selects all events.
    pub(crate) fn is_faking(&self) -> bool {
        self.active_fake(&self.read()).is_some()
    }

    pub fn fake(&self, events: &[&str]) -> Arc<EventFake> {
        self.install_fake(EventFake::new(events, &[]))
    }

    pub(crate) fn install_fake(&self, fake: Arc<EventFake>) -> Arc<EventFake> {
        if let Some(scoped) = self.scoped_fake() {
            let previous = std::mem::replace(
                &mut *scoped
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                fake.clone(),
            );
            drop(previous);
            return fake;
        }
        let previous = self.write().fake.replace(fake.clone());
        drop(previous);
        fake
    }

    pub(crate) async fn with_fake<T>(
        &self,
        fake: Arc<EventFake>,
        callback: impl Future<Output = EventResult<T>>,
    ) -> EventResult<T> {
        let mut scopes = SCOPED_FAKES.try_with(Vec::clone).unwrap_or_default();
        scopes.push(ScopedFake {
            dispatcher: self.registrations.clone(),
            fake: Arc::new(Mutex::new(fake)),
        });
        SCOPED_FAKES.scope(scopes, callback).await
    }

    fn scoped_fake(&self) -> Option<Arc<Mutex<Arc<EventFake>>>> {
        SCOPED_FAKES
            .try_with(|scopes| {
                scopes
                    .iter()
                    .rev()
                    .find(|scope| Arc::ptr_eq(&scope.dispatcher, &self.registrations))
                    .map(|scope| scope.fake.clone())
            })
            .unwrap_or_default()
    }

    fn active_fake(&self, registrations: &Registrations) -> Option<Arc<EventFake>> {
        self.scoped_fake()
            .map(|scope| {
                scope
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            })
            .or_else(|| registrations.fake.clone())
    }

    async fn dispatch_event(
        &self,
        event: Arc<dyn Event>,
        context: &mut EventContext<'_>,
        halt: bool,
        always_defer: bool,
    ) -> EventResult<DispatchResult> {
        if context.active_transaction().is_none()
            && DEFERRED_EVENTS
                .try_with(|scope| {
                    scope.as_ref().is_some_and(|scope| {
                        if !Arc::ptr_eq(&scope.dispatcher, &self.registrations)
                            || scope
                                .names
                                .as_ref()
                                .is_some_and(|names| !names.contains(event.name()))
                        {
                            return false;
                        }
                        scope.events.borrow_mut().push(DeferredEvent {
                            event: event.clone(),
                            halt,
                            after_commit: always_defer,
                            socket: context.socket.clone(),
                        });
                        true
                    })
                })
                .unwrap_or(false)
        {
            return Ok(DispatchResult::Deferred);
        }
        let (fake, after_commit) = {
            let registrations = self.read();
            (
                self.active_fake(&registrations),
                always_defer
                    || registrations
                        .deferred_types
                        .contains(&event.as_ref().type_id()),
            )
        };
        let fake = fake.filter(|fake| fake.matches(event.as_ref()));
        if after_commit {
            let socket = context.socket.clone();
            if let Some(transaction) = context.active_transaction() {
                self.defer_event(transaction, event, halt, socket, fake)?;
                return Ok(DispatchResult::Deferred);
            }
        }
        self.deliver(event, context, halt, fake)
            .await
            .map(DispatchResult::Completed)
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Registrations> {
        self.registrations
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Registrations> {
        self.registrations
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn defer_event(
        &self,
        transaction: &mut Transaction,
        event: Arc<dyn Event>,
        halt: bool,
        socket: Option<String>,
        fake: Option<Arc<EventFake>>,
    ) -> EventResult<()> {
        let dispatcher = self.clone();
        defer(transaction, async move {
            dispatcher
                .deliver(
                    event,
                    &mut EventContext {
                        socket,
                        ..EventContext::new()
                    },
                    halt,
                    fake,
                )
                .await?;
            Ok(())
        })
    }

    async fn broadcast(
        &self,
        event: &dyn Event,
        context: &mut EventContext<'_>,
    ) -> EventResult<()> {
        let registration = self.read().broadcasts.get(&event.type_id()).copied();
        let Some(registration) = registration else {
            return Ok(());
        };
        let source = event;
        let event = (registration.event)(source).ok_or(EventError::InvalidPayload)?;
        let Some(mut delivery) = BroadcastEvent::from_event(event, context.socket.as_deref())
        else {
            return Ok(());
        };
        if registration.immediate || event.should_broadcast_now() {
            let owner = self.owner.upgrade().ok_or_else(|| {
                EventError::Broadcast(BroadcastException::Backend(
                    std::io::Error::other("Broadcast application is unavailable.").into(),
                ))
            })?;
            let factory = owner.make::<dyn BroadcastFactory>()?;
            delivery
                .handle(factory.as_ref())
                .await
                .map_err(EventError::Broadcast)?;
        } else {
            delivery.unique = registration.unique.and_then(|unique| unique(source));
            let payload = crate::queue::Payload::new(JOB, delivery)
                .map_err(EventError::Queue)?
                .with_job_subject(crate::queue::JobType::Broadcast, event.type_name());
            let options = crate::queue::DispatchOptions {
                connection: event.broadcast_queue_connection().map(str::to_owned),
                queue: event.broadcast_queue().map(str::to_owned),
                after_commit: event.broadcast_after_commit(),
                ..Default::default()
            };
            self.queue()?
                .dispatch(payload, options, context.active_transaction())
                .await
                .map_err(EventError::Queue)?;
        }
        Ok(())
    }

    async fn deliver(
        &self,
        event: Arc<dyn Event>,
        context: &mut EventContext<'_>,
        halt: bool,
        fake: Option<Arc<EventFake>>,
    ) -> EventResult<Vec<Option<ListenerResponse>>> {
        if let Some(fake) = fake {
            fake.record(event);
            return Ok(Vec::new());
        }
        self.broadcast(event.as_ref(), context).await?;
        let name = event.name();
        let interfaces = event.interfaces();
        let state = self.pushed_events()?;
        let (pushed, forgotten_through) = {
            let state = state.0.lock().expect("事件暂存锁中毒");
            (
                state.events.get(name).cloned().unwrap_or_default(),
                state.forgotten_through,
            )
        };
        let listeners = self.read().listeners.selected_with_pushed(
            name,
            interfaces,
            &pushed,
            forgotten_through,
        );
        let mut responses = Vec::with_capacity(listeners.len());
        for registration in listeners {
            let after_commit = registration.after_commit;
            let response = match registration.source {
                ListenerSource::Pushed(pushed) => {
                    self.dispatch(pushed, context).await?;
                    None
                }
                ListenerSource::Queued(queued) => {
                    let payload =
                        crate::queue::Payload::new(&queued.name, (queued.encode)(event.as_ref())?)
                            .map_err(EventError::Queue)?
                            .with_job_subject(crate::queue::JobType::Event, queued.subject_type)
                            .with_event_type(event.type_name());
                    let options = crate::queue::DispatchOptions {
                        connection: queued.options.connection.clone(),
                        queue: queued.options.queue.clone(),
                        delay: queued.options.delay.map(crate::queue::Delay::For),
                        after_commit: queued.options.after_commit,
                    };
                    self.queue()?
                        .dispatch(payload, options, context.active_transaction())
                        .await
                        .map_err(EventError::Queue)?;
                    None
                }
                ListenerSource::Instance(listener) => {
                    invoke(listener, &event, context, after_commit).await?
                }
                ListenerSource::Container(resolve) => {
                    let owner = self.owner.upgrade().ok_or(EventError::Unavailable(
                        BindingResolutionError::FacadeNotSet,
                    ))?;
                    application_scope(owner.clone(), async {
                        let listener = resolve(&owner).await?;
                        invoke(listener, &event, context, after_commit).await
                    })
                    .await?
                }
            };
            if halt {
                if response.is_some() {
                    return Ok(vec![response]);
                }
            } else {
                if matches!(response, Some(ListenerResponse::Stop)) {
                    break;
                }
                responses.push(response);
            }
        }
        Ok(responses)
    }
}

impl Contract for Dispatcher {
    fn listen(
        &self,
        event: &str,
        listener: Arc<dyn Listener>,
        delivery: Delivery,
    ) -> EventResult<()> {
        self.register(|registry| registry.listen(event, listener, delivery))
    }

    fn has_listeners(&self, event: &str) -> bool {
        let Ok(state) = self.pushed_events() else {
            return false;
        };
        let state = state.0.lock().expect("事件暂存锁中毒");
        !self
            .read()
            .listeners
            .selected_with_pushed(
                event,
                &[],
                state.events.get(event).map_or(&[], Vec::as_slice),
                state.forgotten_through,
            )
            .is_empty()
    }

    fn subscribe(&self, subscriber: &dyn Subscriber) -> EventResult<()> {
        self.register(|registry| subscriber.subscribe(registry))
    }

    fn forget(&self, event: &str) {
        if let Ok(state) = self.pushed_events() {
            state.0.lock().expect("事件暂存锁中毒").events.remove(event);
        }
        let removed = self.write().listeners.forget(event);
        drop(removed);
    }

    fn dispatch<'a>(
        &'a self,
        event: Arc<dyn Event>,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<DispatchResult>> {
        Box::pin(self.dispatch_event(event, context, false, false))
    }

    fn until<'a>(
        &'a self,
        event: Arc<dyn Event>,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            match self.dispatch_event(event, context, true, false).await? {
                DispatchResult::Completed(responses) => Ok(responses.into_iter().next().flatten()),
                DispatchResult::Deferred => Ok(None),
            }
        })
    }
}

async fn invoke(
    listener: Arc<dyn Listener>,
    event: &Arc<dyn Event>,
    context: &mut EventContext<'_>,
    after_commit: bool,
) -> EventResult<Option<ListenerResponse>> {
    if after_commit && let Some(transaction) = context.active_transaction() {
        let event = event.clone();
        defer(transaction, async move {
            listener
                .handle(event.as_ref(), &mut EventContext::new())
                .await?;
            Ok(())
        })?;
        return Ok(None);
    }
    listener.handle(event.as_ref(), context).await
}

fn defer(
    transaction: &mut Transaction,
    callback: impl Future<Output = EventResult<()>> + Send + 'static,
) -> EventResult<()> {
    let app = current_container();
    transaction
        .after_commit(move || {
            Box::pin(async move {
                let result = if let Some(app) = app {
                    application_scope(app, callback).await
                } else {
                    callback.await
                };
                result.map_err(|source| DatabaseError::AfterCommit {
                    source: Box::new(source),
                })
            })
        })
        .map_err(EventError::Transaction)
}

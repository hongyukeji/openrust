use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock, Weak},
};

use crate::{
    broadcasting::{BroadcastEvent, BroadcastException, JOB},
    container::Container,
    contracts::broadcasting::{Factory as BroadcastFactory, ShouldBroadcast, ShouldBroadcastNow},
    contracts::container::Container as _,
    contracts::events::{
        Dispatcher as Contract, Event, Listener, ShouldDispatchAfterCommit, Subscriber,
    },
    database::{DatabaseError, Transaction},
    support::facades::{application_scope, current_container},
};

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

type BroadcastSource = for<'a> fn(&'a dyn Event) -> Option<&'a dyn ShouldBroadcast>;

#[derive(Clone, Copy)]
struct BroadcastRegistration {
    event: BroadcastSource,
    immediate: bool,
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

tokio::task_local! {
    static DEFERRED_EVENTS: Option<DeferredEvents>;
}

/// Registers and dispatches events to listeners in one application.
#[derive(Clone, Default)]
pub struct Dispatcher {
    registrations: Arc<RwLock<Registrations>>,
    owner: Weak<Container>,
}

#[allow(non_snake_case, reason = "Matches Laravel's dispatcher names")]
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

    fn queue(&self) -> EventResult<Arc<crate::queue::QueueManager>> {
        self.owner
            .upgrade()
            .ok_or(EventError::QueueUnavailable)?
            .make::<crate::queue::QueueManager>()
            .map_err(EventError::Unavailable)
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

    pub fn markAfterCommit<E: ShouldDispatchAfterCommit>(&self) {
        self.write().deferred_types.insert(TypeId::of::<E>());
    }

    /// Registers the broadcast contract for this compiled event type.
    pub fn registerBroadcast<E: ShouldBroadcast>(&self) {
        self.register_broadcast::<E>(false);
    }

    /// Registers the synchronous broadcast contract for this compiled event type.
    pub fn registerBroadcastNow<E: ShouldBroadcastNow>(&self) {
        self.register_broadcast::<E>(true);
    }

    fn register_broadcast<E: ShouldBroadcast>(&self, immediate: bool) {
        self.write().broadcasts.insert(
            TypeId::of::<E>(),
            BroadcastRegistration {
                event: |event| {
                    event
                        .downcast_ref::<E>()
                        .map(|event| event as &dyn ShouldBroadcast)
                },
                immediate,
            },
        );
    }

    pub fn hasListeners(&self, event: &str) -> bool {
        self.has_listeners(event)
    }

    pub(crate) fn observes(&self, name: &str) -> bool {
        let registrations = self.read();
        registrations.listeners.has_listeners(name)
            || registrations
                .fake
                .as_ref()
                .is_some_and(|fake| fake.matches(name))
    }

    pub fn listing(&self) -> Vec<(String, Vec<&'static str>)> {
        self.read().listeners.listing()
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

    pub async fn dispatchAfterCommit(
        &self,
        event: impl ShouldDispatchAfterCommit,
        context: &mut EventContext<'_>,
    ) -> EventResult<DispatchResult> {
        self.dispatch_event(Arc::new(event), context, false, true)
            .await
    }

    /// Records the selected events for this application. An empty slice selects all events.
    pub fn fake(&self, events: &[&str]) -> Arc<EventFake> {
        let fake = Arc::new(EventFake::new(events));
        let previous = self.write().fake.replace(fake.clone());
        drop(previous);
        fake
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
                registrations.fake.clone(),
                always_defer
                    || registrations
                        .deferred_types
                        .contains(&event.as_ref().type_id()),
            )
        };
        let fake = fake.filter(|fake| fake.matches(event.name()));
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
        let event = (registration.event)(event).ok_or(EventError::InvalidPayload)?;
        let Some(delivery) = BroadcastEvent::from_event(event, context.socket.as_deref()) else {
            return Ok(());
        };
        if registration.immediate {
            let owner = self.owner.upgrade().ok_or_else(|| {
                EventError::Broadcast(BroadcastException::Backend(
                    std::io::Error::other("Broadcast application is unavailable.").into(),
                ))
            })?;
            let factory = owner
                .make::<dyn BroadcastFactory>()
                .map_err(EventError::Unavailable)?;
            delivery
                .handle(factory.as_ref())
                .await
                .map_err(EventError::Broadcast)?;
        } else {
            let payload = crate::queue::Payload::new(JOB, delivery).map_err(EventError::Queue)?;
            let options = crate::queue::DispatchOptions {
                connection: event.broadcastQueueConnection().map(str::to_owned),
                queue: event.broadcastQueue().map(str::to_owned),
                after_commit: event.broadcastAfterCommit(),
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
        let listeners = self.read().listeners.selected(event.name());
        let mut responses = Vec::with_capacity(listeners.len());
        for registration in listeners {
            let response = if let Some(queued) = registration.queued {
                let payload =
                    crate::queue::Payload::new(&queued.name, (queued.encode)(event.as_ref())?)
                        .map_err(EventError::Queue)?;
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
            } else if registration.after_commit
                && let Some(transaction) = context.active_transaction()
            {
                let event = event.clone();
                defer(transaction, async move {
                    registration
                        .listener
                        .handle(event.as_ref(), &mut EventContext::new())
                        .await?;
                    Ok(())
                })?;
                None
            } else {
                registration
                    .listener
                    .handle(event.as_ref(), context)
                    .await?
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
        self.read().listeners.has_listeners(event)
    }

    fn subscribe(&self, subscriber: &dyn Subscriber) -> EventResult<()> {
        self.register(|registry| subscriber.subscribe(registry))
    }

    fn forget(&self, event: &str) {
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

fn defer(
    transaction: &mut Transaction,
    callback: impl Future<Output = EventResult<()>> + Send + 'static,
) -> EventResult<()> {
    let app = current_container();
    transaction
        .afterCommit(move || {
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

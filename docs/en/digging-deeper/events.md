# Events

## Generating Events and Listeners {#generating-events-and-listeners}

Generate event and listener modules with Artisan. Add `--event` to type the handler, `--queued` to dispatch through a queue, and `--test` to generate a matching feature test for a listener. Existing files are preserved unless you pass `--force`.

```sh
./artisan make:event OrderPlaced
./artisan make:listener LogOrder
./artisan make:listener RecordOrder --event=OrderPlaced --test
./artisan make:listener QueueOrder --queued
./artisan make:listener NotifyOrder --event=OrderPlaced --queued
```

Pass a PascalCase event name such as `OrderPlaced`, an application-qualified name such as `App/Events/OrderPlaced`, or a Rust path such as `crate::app::events::order_placed::OrderPlaced` to `--event`.

Declare the generated files in your Rust module tree and pass `openrust::foundation::events::DiscoverEvents::new(&[register_orders])` to `withEvents` in your application builder. The queued listener without `--event` takes the event type at registration. Generated events derive Serde serialization for use in the queue.

```rust
use crate::app::{
    events::order_placed::OrderPlaced,
    listeners::{log_order, notify_order, queue_order, record_order},
};
use openrust::events::{EventResult, ListenerRegistry};

fn register_orders(registry: &mut ListenerRegistry) -> EventResult<()> {
    log_order::register(registry, OrderPlaced::NAME)?;
    record_order::register(registry, OrderPlaced::NAME)?;
    queue_order::register::<OrderPlaced>(registry, OrderPlaced::NAME)?;
    notify_order::register(registry, OrderPlaced::NAME)
}
```

## Registering listeners and subscribers

Declare event registration functions in your application and pass them to `withEvents(DiscoverEvents::new(&[register_orders]))`. The application loads this compiled manifest during boot. Implement `Subscriber` and call `Event::subscribe` to register related listeners together. Use `./artisan event:list --event=orders --json` to inspect the resulting registrations.

Register a listener type with `registry.listenClass::<OrderListener>("orders.created", Delivery::Sync)?` in the manifest, or use `Event::listenClass` in the application scope. Bind the type with an application container factory, such as `bindAsync` for asynchronous dependencies. The dispatcher resolves it when the event reaches that listener, using the owning application's container. A missing binding fails the dispatch. With `Delivery::AfterCommit`, dispatch resolves the instance immediately and a successful commit invokes it.

For dynamic discovery, implement `ShouldBeDiscovered` and attach the listener registration with `withListener`. Pass the resulting manifest to `withEvents(discover_events())`. During boot, the policy reads the current application and decides whether to include that listener. Returning an error aborts the manifest registration. Manual registration through `Event::listen` is independent of discovery.

```rust
use std::sync::Arc;

use openrust::{
    contracts::events::{Event, Listener, ShouldBeDiscovered},
    events::{
        Delivery, EventContext, EventError, EventFuture, EventResult,
        ListenerRegistry, ListenerResponse, NamedEvent,
    },
    foundation::{Application, events::DiscoverEvents},
};

struct OrderListener;

impl ShouldBeDiscovered for OrderListener {
    fn shouldBeDiscovered(app: &Application) -> EventResult<bool> {
        app.environment(Some("production".into()))
            .map(bool::from)
            .map_err(EventError::Unavailable)
    }
}

impl Listener for OrderListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event.downcast_ref::<NamedEvent<u64>>()
                .ok_or(EventError::InvalidPayload)?;
            Ok(Some(ListenerResponse::Value(Box::new(*event.payload()))))
        })
    }
}

fn register_orders(registry: &mut ListenerRegistry) -> EventResult<()> {
    registry.listen("orders.created", Arc::new(OrderListener), Delivery::Sync)
}

fn discover_events() -> DiscoverEvents {
    DiscoverEvents::new(&[]).withListener::<OrderListener>(register_orders)
}
```

### Event discovery in production {#event-discovery-in-production}

Run `./artisan event:cache` in the deployment environment to cache the compiled manifest’s discovery decisions. A new process uses those decisions and runs the selected compiled registrars. Manual listeners and unconditional registrations run as usual. The cache lives at `storage/framework/cache/events.json`. Refresh it after deploying a new build or changing discovery configuration. Run `./artisan event:clear` to resume live discovery on the next boot. A malformed cache or a mismatched listener list falls back to live discovery.

For model lifecycle handlers, register an [Eloquent observer](../eloquent/getting-started.md#observers) during provider boot.

## Dispatching events

Call `Event::dispatch(NamedEvent::new("orders.created", 42_u64)).await?` or dispatch a type that implements `contracts::events::Event`. Exact registrations run in registration order, followed by matching wildcard groups. `ListenerResponse::Stop` stops propagation. `Event::until` returns the first listener response.

Typed events may declare shared contract names through `Event::interfaces()`. Register a listener under a contract name with `Event::listen`. For example, an order event can return `&["orders.lifecycle"]` to share a listener with other order events. These exact contract listeners run after the event’s exact and wildcard listeners, in the declared contract order. Each handler receives the original payload. Wildcard matching uses the dispatched event name.

## Dispatching after a database transaction

Pass the active transaction to `Event::dispatchInTransaction`. Use `Delivery::AfterCommit` for a single listener, or implement `ShouldDispatchAfterCommit` and call `Event::dispatchAfterCommit` to defer the entire event. A successful savepoint keeps its callbacks for the outer commit. Rolling back discards callbacks from the affected scope. Without an active transaction the listener runs immediately. A callback error after commit is reported as `DatabaseError::AfterCommit`.

See [observers and database transactions](../eloquent/getting-started.md#observers-and-database-transactions) for model-specific callbacks.

## Deferring events {#deferring-events}

Use `Event::defer(None, async { … })` to collect events and deliver them in order when the callback returns `Ok`. Pass `Some(&["orders.created"])` to defer selected event names. Returning an error discards pending events. The scope belongs to the current async task and application. For dispatch with an explicitly supplied database transaction, use the transaction APIs above.

## Testing {#testing}

`TestApp::fakeEvents` records selected dispatches in the fixture. Inside an active application scope you can also use `Event::fake`. Use an empty slice to select every event. Pass exact event names to limit the fake, or call `except` to let selected events reach their listeners.

```rust
use openrust::{
    contracts::events::Dispatcher as _,
    events::{Dispatcher, EventContext, NamedEvent},
    testing::TestApp,
};
use std::sync::Arc;

#[tokio::test]
async fn records_order_events() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    let fake = app.fakeEvents(&["orders.created"]).await?;
    let dispatcher = app.app().make::<Dispatcher>().await?;
    dispatcher.dispatch(
        Arc::new(NamedEvent::new("orders.created", 42_u64)),
        &mut EventContext::new(),
    ).await?;

    fake.assertDispatchedTimes("orders.created", 1)
        .assertNotDispatched("orders.cancelled");
    Ok(())
}
```

Faked events skip listeners, queueing and broadcasting. Events deferred until commit are recorded only after that commit. Rollback discards them. `dispatched` exposes recorded payloads for typed inspection. The fake lasts for the fixture or until another fake is installed.

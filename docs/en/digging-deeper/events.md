# Events

## Registering listeners and subscribers

Declare event registration functions in your application and pass them to `withEvents(DiscoverEvents::new(&[register_orders]))`. The application loads this compiled manifest during boot. Implement `Subscriber` and call `Event::subscribe` to register related listeners together. Use `./artisan event:list --event=orders --json` to inspect the resulting registrations.

```rust
use std::sync::Arc;

use openrust::{
    contracts::events::{Event, Listener},
    events::{
        Delivery, EventContext, EventError, EventFuture, EventResult,
        ListenerRegistry, ListenerResponse, NamedEvent,
    },
};

struct OrderListener;

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
```

## Dispatching events

Call `Event::dispatch(NamedEvent::new("orders.created", 42_u64)).await?` or dispatch a type that implements `contracts::events::Event`. Exact registrations run in registration order, followed by matching wildcard groups. `ListenerResponse::Stop` stops propagation. `Event::until` returns the first listener response.

## Dispatching after a database transaction

Pass the active transaction to `Event::dispatchInTransaction`. Use `Delivery::AfterCommit` for a single listener, or implement `ShouldDispatchAfterCommit` and call `Event::dispatchAfterCommit` to defer the entire event. A successful savepoint keeps its callbacks for the outer commit. Rolling back discards callbacks from the affected scope. Without an active transaction the listener runs immediately. A callback error after commit is reported as `DatabaseError::AfterCommit`.

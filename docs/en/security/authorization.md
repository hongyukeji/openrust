# Authorization

## Gates

Register abilities with the application’s `GateRegistry`. `define` requires an authenticated user. Use `defineGuest` when the ability should also evaluate guests.

```rust
use openrust::auth::{AuthIdentifier, access::{GateRegistry, Response}};

fn register_settings_gate(gate: &GateRegistry) {
    gate.define("edit-settings", |user, _| {
        Box::pin(async move {
            Ok(Response::from(
                user.getAuthIdentifier() == AuthIdentifier::String("owner".into()),
            ))
        })
    });
}
```

## Policies

Generate a policy with `./artisan make:policy AccountPolicy --model=User`. Add the generated module to the application and register the policy with `gate.policy::<User>(AccountPolicy)` in a service provider. Policies select the model by its Rust type. For an ability without a model instance, pass `PolicyTarget::of::<User>()`.

## Authorizing requests

Use `Gate::authorize(request, "edit-settings", &[]).await?` in a handler or attach `.can("edit-settings", &[])` to the route. For model policies, `.can("update", &["user"])` receives the route-bound `user`. Middleware runs authorization after authentication and model binding.

Use the same Gate abilities to conditionally render controls in [Blade authorization directives](../the-basics/blade#authorization-directives).

## Decisions and hooks

`inspect` returns the authorization decision. A refusal returns HTTP 403 by default. Use `Response::denyWithStatus` or `Response::denyAsNotFound` to change the status. `before` may decide early. `after` may supply a decision when the ability and `before` left it unresolved.

## Gate evaluation event {#gate-evaluated}

The application dispatcher emits `GateEvaluated` after the Gate’s `after` callbacks and before it applies a default denial. The event contains the optional user, the ability, and the raw `Option<Response>` result, including any explicit denial status. An unresolved ability has a `None` result even if `inspect` returns a configured denial. Each ability that actually runs emits its own event. Inline checks with `allowIf` and `denyIf` skip this event.

Register a synchronous listener to read the original arguments in order through `event.arguments(context)`. Deferred delivery and retained events expose `argument_types`, since the original arguments are borrowed for the duration of the call.

```rust
use std::sync::Arc;

use openrust::{
    auth::access::events::GateEvaluated,
    contracts::events::{Dispatcher as _, Event, Listener},
    events::{Delivery, Dispatcher, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
};

struct RecordGate;

impl Listener for RecordGate {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event.downcast_ref::<GateEvaluated>().ok_or(EventError::InvalidPayload)?;
            let allowed = event.result.as_ref().map(|result| result.allowed());
            let argument_count = event.arguments(context).map(|arguments| arguments.len());
            eprintln!("{} {allowed:?} {argument_count:?}", event.ability);
            Ok(None)
        })
    }
}

fn register_gate_listener(events: &Dispatcher) -> EventResult<()> {
    events.listen("GateEvaluated", Arc::new(RecordGate), Delivery::Sync)
}
```


## Response status

Use `Response::deny("reason").withStatus(StatusCode::CONFLICT)` to set a denial status, or `asNotFound()` to select 404. `withStatus(None)` clears a previously selected status. These consuming builders retain the authorization decision and message; setting a status on an allowed response does not deny it.

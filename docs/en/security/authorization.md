# Authorization

## Gates

Register abilities with the application’s `GateRegistry`. `define` requires an authenticated user. Use `define_guest` when the ability should also evaluate guests.

```rust
use openrust::auth::{AuthIdentifier, access::{GateRegistry, Response}};

fn register_settings_gate(gate: &GateRegistry) {
    gate.define("edit-settings", |user, _| {
        Box::pin(async move {
            Ok(Response::from(
                user.get_auth_identifier() == AuthIdentifier::String("owner".into()),
            ))
        })
    });
}
```

## Policies

Generate a policy with `./artisan make:policy AccountPolicy --model=User`. Add the generated module to the application and register the policy with `gate.policy::<User>(AccountPolicy)` in a service provider. Policies select the model by its Rust type. For an ability without a model instance, pass `PolicyTarget::of::<User>()`.

## Policy discovery and lookup

`get_policy_for` accepts a model instance or `PolicyTarget::of::<User>()` and returns `Result<Option<PolicyHandle>>`. The handle retains the actual policy instance: `downcast::<AccountPolicy>()` retrieves its `Arc<AccountPolicy>`, and `name()` returns its full Rust type name. Explicit `policy` mappings take precedence. `policies()` returns an independent snapshot keyed by model `TypeId`, with policy type names as values.

To discover policies, register their construction factories with the application container, then register compiled types with `discover_policy::<User, AccountPolicy>()`. This does not construct policies or add explicit mappings. Default lookup searches progressively enclosing model modules for `policies::<ModelName>Policy`; nested `models` modules prefer the corresponding `models::policies` and `policies` paths. For separate file modules or another layout, return actual full type names from `guess_policy_names_using`. The first registered candidate wins; an empty list disables default discovery.

```rust
use std::{any::type_name, sync::Arc};
use openrust::{
    auth::access::PolicyTarget,
    contracts::container::Container as _,
    support::facades::Gate,
};

container.bind::<AccountPolicy, _>(|_| Ok(Arc::new(AccountPolicy)))?;
Gate::discover_policy::<User, AccountPolicy>()?;
Gate::guess_policy_names_using(|model| {
    if model == type_name::<User>() {
        vec![type_name::<AccountPolicy>().into()]
    } else {
        Vec::new()
    }
})?;
let selected = Gate::get_policy_for(&PolicyTarget::of::<User>())?;
let concrete = selected.and_then(|policy| policy.downcast::<AccountPolicy>());
let resolved = Gate::resolve_policy::<AccountPolicy>()?;
```

Use these calls inside a provider or another application Facade scope. `container` is that application's container, and `User` and `AccountPolicy` are application types. Resolve constructor dependencies inside the container factory. `resolve_policy::<AccountPolicy>()` uses existing container bindings directly; discovery respects the same transient, shared and failure behavior. Construction failures propagate without trying later candidates. A decision from `before` skips policy construction.

`GateRegistry` provides the same methods. The Gate returned by `for_user` exposes policy lookup, resolution, mappings and the guessing callback; the Gate contract exposes `get_policy_for`. Explicit `policy` registration continues to share the supplied instance. Discovery only handles registered compiled types: PHP class autoloading, attribute reflection and parent-class lookup are unavailable. Results use owned handles and `Result`; mapping snapshots do not guarantee iteration order.

## Authorizing requests

Use `Gate::authorize(request, "edit-settings", &[]).await?` in a handler or attach `.can("edit-settings", &[])` to the route. For model policies, `.can("update", &["user"])` receives the route-bound `user`. Middleware runs authorization after authentication and model binding.

Use the same Gate abilities to conditionally render controls in [Blade authorization directives](../the-basics/blade#authorization-directives).

## Decisions and hooks

`inspect` returns the authorization decision. A refusal returns HTTP 403 by default. Use `Response::deny_with_status` or `Response::deny_as_not_found` to change the status. `before` may decide early. `after` may supply a decision when the ability and `before` left it unresolved.

## Gate evaluation event {#gate-evaluated}

The application dispatcher emits `GateEvaluated` after the Gate’s `after` callbacks and before it applies a default denial. The event contains the optional user, the ability, and the raw `Option<Response>` result, including any explicit denial status. An unresolved ability has a `None` result even if `inspect` returns a configured denial. Each ability that actually runs emits its own event. Inline checks with `allow_if` and `deny_if` skip this event.

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

Use `Response::deny("reason").with_status(StatusCode::CONFLICT)` to set a denial status, or `as_not_found()` to select 404. `with_status(None)` clears a previously selected status. These consuming builders retain the authorization decision and message; setting a status on an allowed response does not deny it.

# Authorization

## Gates

Register abilities with the application’s `GateRegistry`. `define` requires an authenticated user. Use `defineGuest` when the ability should also evaluate guests.

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

## Authorizing requests

Use `Gate::authorize(request, "edit-settings", &[]).await?` in a handler or attach `.can("edit-settings", &[])` to the route. For model policies, `.can("update", &["user"])` receives the route-bound `user`. Middleware runs authorization after authentication and model binding.

## Decisions and hooks

`inspect` returns the authorization decision. A refusal returns HTTP 403 by default. Use `Response::denyWithStatus` or `Response::denyAsNotFound` to change the status. `before` may decide early. `after` may supply a decision when the ability and `before` left it unresolved.

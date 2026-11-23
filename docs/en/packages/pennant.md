# Pennant

## Installation {#installation}

Enable the `pennant` feature in the application Cargo.toml. To use `./artisan`, add `"pennant"` to `[features].default`, preserving existing features. Package discovery loads `PennantServiceProvider`; the default application needs no extra Provider registration.

## Configuration {#configuration}

The default store is `database`, selected with `PENNANT_STORE`; `array` keeps states within the current lifecycle. The database table is `features` with a unique feature name and scope. Publish configuration when customization is needed:

```sh
./artisan vendor:publish --tag=pennant-config
./artisan migrate
```

Configuration belongs to the package. Published Rust configuration must be registered using the compiled configuration loader, then rebuilt; publishing alone does not execute a new Rust file. Cached configuration uses its stored values and skips package environment reads. The Provider registers the default table migration without querying the database during startup. For a custom table or connection, supply the matching application migration.

## Defining features {#defining-features}

Register definitions in an application Provider after Pennant has booted. A resolver receives an owned `Scope` and returns a JSON value. `define_value` defines a constant. Definitions are shared within the application; resolved values belong to the current HTTP request, Artisan call or queue job.

```rust
let store = app.make::<openrust_pennant::FeatureManager>().await?.store(None)?;
store.define_value("new-api", serde_json::json!(true))?;
store.define("theme", std::sync::Arc::new(|scope| Box::pin(async move {
    Ok(serde_json::json!(if scope == openrust_pennant::Scope::Null { "default" } else { "blue" }))
})))?;
```

## Checking features {#checking-features}

Only JSON `false` is inactive. `null`, zero, empty strings and arrays are active values. An unknown feature resolves to false and emits `UnknownFeatureResolved`; this fallback is not persisted. `all_are_active` requires every feature in every scope; `some_are_active` requires at least one matching feature in each scope.

```rust
let user = store.r#for("user-42");
let enabled = user.active("new-api").await?;
let theme = user.value("theme").await?;
user.load_missing(&["new-api", "theme"]).await?;
let both = user.all_are_active(&["new-api", "theme"]).await?;
```

## Scopes {#scopes}

Use an explicit scope with `for`, `Scope::from_model` for an Eloquent model, or `globally` for a single global scope. Global and anonymous scopes are distinct. Models use the concrete Rust class and primary key; `use_morph_map(true)` switches the class portion to its configured morph alias. `FeatureScopeSerializeable` and `Scope::from_serializable` support custom serialization; `FeatureScopeable` and `for_scopeable` select an identifier for the chosen store.

The default scope reads the identity already resolved by native authentication. For a lazy guard, call `Feature::for_request(request).await?` before checking features, or run authentication middleware first. `EnsureFeaturesAreActive` resolves the default guard itself. Outside HTTP, use an explicit scope or configure `FeatureManager::resolve_scope_using`.

## Feature classes {#feature-classes}

Implement `FeatureDefinition`, then register an asynchronous factory with `define_class`. The factory may resolve container dependencies. `name` supplies a custom name; `accepts_null_scope` explicitly replaces PHP parameter reflection. A non-null result from `before` overrides stored values without persisting it. Values already cached take precedence; `flush_cache` lets the hook run again. Returning `None` from `before` delegates to the driver.

```rust
use openrust_pennant::{FeatureDefinition, FeatureFuture, Scope};
use serde_json::Value;
use std::sync::Arc;

struct NewApi;
impl FeatureDefinition for NewApi {
    fn name() -> &'static str { "new-api" }
    fn resolve<'a>(&'a self, _scope: &'a Scope) -> FeatureFuture<'a, Value> {
        Box::pin(async { Ok(Value::Bool(true)) })
    }
}
// Application Provider:
store.define_class(|| Box::pin(async { Ok(Arc::new(NewApi)) }))?;
```

Rust modules and factories are registered at compile time. Runtime PHP class discovery, reflection and macros are not available. Enums can pass their explicit string representation. Factories are lazy; defining a feature does not resolve its dependencies.

`store.discover` accepts compiled registration functions. Use `store.instance::<NewApi>().await?` to obtain a typed class instance with application-specific methods. `is_valid_for_scope` controls which definitions `all` and `load_all` include. Eloquent models gain `features(store)` by importing `openrust_pennant::concerns::HasFeatures`. `define_lottery` reuses the native `Lottery` and stores the first resolved result like other definitions.

## Updating values {#updating-values}

Activation writes to the selected scopes. `activate_for_everyone` updates existing stored scopes; newly encountered scopes still use their resolver. `forget` deletes a state so it can resolve again. The array driver clears its resolved states with `flush_cache`; the database driver retains stored states.

```rust
user.activate("new-api").await?;
user.activate_with_value(&["theme"], serde_json::json!("green")).await?;
user.deactivate("new-api").await?;
user.forget(&["theme"]).await?;
store.activate_for_everyone("new-api", serde_json::json!(true)).await?;
```

## Middleware {#middleware}

Add `EnsureFeaturesAreActive::using(["new-api"])` to a native route group using `Arc::new(...)`. Inactive features return HTTP 400. `when_inactive` accepts a per-instance asynchronous response callback. Configure authentication/session middleware appropriate to the route.

## Commands {#commands}

`pennant:feature` creates a Rust class under `app/features`; declare its module and register the factory in your Provider. `stubs/feature.rs.stub` overrides the template. Existing files are preserved unless `--force` is supplied. Purge commands delete stored values while retaining definitions; `pennant:clear` aliases `pennant:purge`. Use deployed commands through `./main artisan`.

```sh
./artisan pennant:feature NewApi
./artisan pennant:purge new-api theme
./artisan pennant:purge --except=new-api --except=theme
./artisan pennant:purge --except-registered --store=database
./artisan pennant:clear
```

## Custom drivers and events {#drivers-and-events}

Use `FeatureManager::extend` to register a creator for a configured driver. Implement the native `Driver` contract, including bulk loading. `stored` is optional and reports an unsupported operation by default; enable `supports_set_all` only when implementing a bulk writer. Otherwise updates run one by one with an event after each write. Creators must not recursively resolve their own store. Events use the native Dispatcher: `FeatureResolved`, `FeatureRetrieved`, `UnknownFeatureResolved`, `UnexpectedNullScopeEncountered`, `FeatureUpdated`, `FeatureUpdatedForAllScopes`, `FeatureDeleted`, `FeaturesPurged` and `AllFeaturesPurged`. Retrieval events include cache hits; listener failures propagate to the caller.

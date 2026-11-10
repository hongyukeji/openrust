# Facades

## How Facades Work

Import `openrust::support::facades::Facade` to define a facade's service type and `getFacadeAccessor`. `getFacadeRoot` resolves the accessor through the current application's container. Define explicit forwarding methods instead of PHP's `__callStatic`; Rust checks those methods at compilation.

```rust
use openrust::{
    container::ServiceKey,
    contracts::container::BindingResolutionError,
    support::facades::Facade,
};

struct Messages;
impl Facade for Messages {
    type Service = str;
    fn getFacadeAccessor() -> ServiceKey { "message".into() }
}
impl Messages {
    pub fn text() -> Result<String, BindingResolutionError> {
        Ok(Self::getFacadeRoot()?.to_string())
    }
}
```

Bind `str` and alias it as `message` in a provider, then call `Messages::text()` inside a provider hook, HTTP handler or Artisan command. Outside a framework application scope the method returns `FacadeNotSet`.

Facade resolutions are cached within the current application execution. Use `clearResolvedInstance(None)` to clear the facade's accessor, `clearResolvedInstance(Some(key))` for a particular accessor, or `clearResolvedInstances()` for all current cached roots. `swap(instance)` replaces both the cached root and its container binding. A facade can set `const cached: bool = false` to avoid caching new resolutions.

The framework scopes facade state to bootstrap, each HTTP request and each Artisan invocation. Nested execution in the same application shares the scope; different applications remain isolated. A new request or command starts a fresh facade cache. A service registered as a singleton still belongs to its application's container. Tokio tasks created by application code do not automatically inherit an ambient facade scope; pass resolved services into those tasks.

`Route::get` and `Artisan::command` use dedicated registration scopes while the route modules' `register()` functions execute. These are the delivered registration facades. The presence of other facade type declarations does not imply their backing services are implemented. Mocking helpers and resolution callbacks are delivered by later units.

Use `getFacadeRootAsync().await` when the root service belongs to an unloaded deferred provider. It waits for readiness and uses the same application-local facade cache. Synchronous root resolution returns an async-required error while the provider is pending. See [deferred providers](providers.md#deferred-providers).

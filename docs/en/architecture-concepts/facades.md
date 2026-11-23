# Facades

## How Facades Work

Import `openrust::support::facades::Facade` to define a facade's service type and `get_facade_accessor`. `get_facade_root` resolves the accessor through the current application's container. Define explicit forwarding methods instead of PHP's `__callStatic`; Rust checks those methods at compilation.

```rust
use openrust::{
    container::ServiceKey,
    contracts::container::BindingResolutionError,
    support::facades::Facade,
};

struct Messages;
impl Facade for Messages {
    type Service = str;
    fn get_facade_accessor() -> ServiceKey { "message".into() }
}
impl Messages {
    pub fn text() -> Result<String, BindingResolutionError> {
        Ok(Self::get_facade_root()?.to_string())
    }
}
```

Bind `str` and alias it as `message` in a provider, then call `Messages::text()` inside a provider hook, HTTP handler or Artisan command. Outside a framework application scope the method returns `FacadeNotSet`.

Facade resolutions are cached within the current application execution. Use `clear_resolved_instance(None)` to clear the facade's accessor, `clear_resolved_instance(Some(key))` for a particular accessor, or `clear_resolved_instances()` for all current cached roots. `swap(instance)` replaces both the cached root and its container binding. A facade can set `const CACHED: bool = false` to avoid caching new resolutions.

The framework scopes facade state to bootstrap, each HTTP request and each Artisan invocation. Nested execution in the same application shares the scope; different applications remain isolated. A new request or command starts a fresh facade cache. A service registered as a singleton still belongs to its application's container. Tokio tasks created by application code do not automatically inherit an ambient facade scope; pass resolved services into those tasks.

`Route::get` and `Artisan::command` use dedicated registration scopes while the route modules' `register()` functions execute. Resolve service facades within an application lifecycle after registering their backing services.

Use `get_facade_root_async().await` for an unloaded deferred provider or an asynchronous factory. It resolves once through the asynchronous container path, waits for readiness and shares the application's facade cache. A newer `swap` or concurrently cached root is retained. Synchronous lookup of a pending provider or uncached async factory returns a typed async-required error.

A pending facade future belongs to its originating application execution. Polling it in another request, command or application returns `FacadeNotSet`. Lifecycle completion clears the cache; a retained container resolution cannot reopen that closed scope (`ScopeClosed`). Dropping work restores its live origin for cleanup and masks ambient facades after the origin closes. Services and futures explicitly retained by application code remain owned by that code. See [asynchronous factories](container.md#asynchronous-factories) and [deferred providers](providers.md#deferred-providers).

`make_with` creates an explicit construction and does not replace a separately cached facade root. Resolve that result directly through the application or container. See [parameter overrides](container.md#parameter-overrides).

# Service Container

## Binding

Register services in a provider's `register` method through `app.container()`. `bind::<T, _>` invokes its factory on every resolution, `singleton::<T, _>` shares the first successful result, and `instance::<T>` supplies an existing `Arc<T>`. Factories receive the same container so they can resolve dependencies explicitly.

Rust does not provide PHP constructor reflection. Bind a trait as `dyn Trait` and return a concrete implementation from its factory. Rust verifies the interface implementation at compilation, while container failures are returned as `BindingResolutionError`. An unregistered type is not implicitly constructed.

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};

trait Mailer: Send + Sync { fn send(&self) -> &'static str; }
struct SmtpMailer;
impl Mailer for SmtpMailer { fn send(&self) -> &'static str { "sent" } }

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singleton::<dyn Mailer, _>(|_| Ok(Arc::new(SmtpMailer)));
    app.alias(ServiceKey::typed::<dyn Mailer>(), "mailer")?;
    let mailer = app.get::<dyn Mailer>("mailer")?;
    assert_eq!(mailer.send(), "sent");
    assert!(Arc::ptr_eq(&mailer, &app.make::<dyn Mailer>()?));
    Ok(())
}
```

### Named Bindings and Aliases

Use `bindNamed::<T, _>(name, factory)`, `singletonNamed::<T, _>(name, factory)` or `instanceNamed::<T>(name, instance)` for named entries. These are the named forms of the same operations: Rust cannot overload a method by its argument count. `get::<T>(name)` resolves a named entry and checks its output type before invoking the factory. Typed and named entries remain distinct even when their displayed names match.

`alias(abstract, alias)` accepts a `ServiceKey::typed::<T>()`, `ServiceKey::named(name)` or a string. Aliases may point to other aliases or to entries registered later. `isAlias` tests the original key; `getAlias` follows the chain; `has` checks a binding or alias without constructing it. Names are exact and case-sensitive. Binding or supplying an instance at an alias key replaces that alias. Rebinding the target affects subsequent resolutions; already returned `Arc` values remain valid.

A direct self-alias is rejected. Alias cycles and factory dependency cycles return errors. A failed or panicking singleton constructor does not cache a value; a later call may retry. Concurrent resolutions share the first successful singleton. Container locks are released before invoking user factories.

## Resolving

Use `make::<T>()` for a Rust type and `get::<T>(key)` for a named or explicit key. The default configuration repository is bound as `dyn openrust::contracts::config::Repository` and aliased as `config`. Every application owns its container; bindings and instances are not process-global.

Deferred services, contextual binding, extenders and resolution callbacks remain in progress in container unit 01-02. The binding and resolving behavior above does not imply those features are implemented.

## Conditional Bindings and Instance State

`bindIf` and `singletonIf` register a typed factory only when the key has no binding, instance or alias. Their named forms are `bindIfNamed` and `singletonIfNamed`. The existence check and insertion are atomic; a rejected factory is not executed and is dropped outside the registry lock. Even an alias pointing to a missing service counts as an existing entry.

`isShared(key)` examines the original key without following aliases. `resolved(key)` follows aliases and returns true for an existing instance or a successfully resolved factory. Resolution history survives instance eviction. Supplying an instance does not execute or discard the original factory.

`forgetInstance(key)` removes the instance for that exact key. It does not follow aliases. A previously registered singleton factory can then construct a fresh instance. `forgetInstances()` clears all existing instances while preserving factories, aliases and resolution history. Instances without a backing factory become unbound; previously returned `Arc` values stay valid.

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{Container as _, BindingResolutionError}};

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonIfNamed::<u32, _>("counter", |_| Ok(Arc::new(7)));
    app.bindIfNamed::<u32, _>("counter", |_| Ok(Arc::new(8)));
    assert!(app.isShared("counter"));
    assert!(!app.resolved("counter")?);
    let original = app.get::<u32>("counter")?;
    app.instanceNamed("counter", Arc::new(99_u32));
    assert_eq!(*app.get::<u32>("counter")?, 99);
    app.forgetInstance("counter");
    let rebuilt = app.get::<u32>("counter")?;
    assert_eq!(*rebuilt, 7);
    assert!(!Arc::ptr_eq(&original, &rebuilt));
    Ok(())
}
```

These operations do not clear a facade's own cached root; use its cache-clearing API when required. Scoped service eviction is described below.

## Scoped Bindings

`scoped::<T, _>` shares a service within one execution lifecycle. Its named form is `scopedNamed::<T, _>(name, factory)`. Use `scopedIf` or `scopedIfNamed` to register only when the original key is unbound. HTTP handler execution and each Artisan dispatch automatically enter a lifecycle. Ordinary `singleton` instances remain shared by the application.

Use the Rust lifecycle helper `Container::scope(future)` for manual execution. Nested calls for the same container reuse its current scope. Concurrent executions have independent caches, including when they run on the same Tokio thread. Different applications keep their own service bindings and instances.

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::Container as _};

fn main() {
    let app = Container::default();
    app.scoped::<u32, _>(|_| Ok(Arc::new(7)));
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        let first = app.scope(async {
            let instance = app.make::<u32>().unwrap();
            assert!(Arc::ptr_eq(&instance, &app.make::<u32>().unwrap()));
            instance
        }).await;
        let second = app.scope(async { app.make::<u32>().unwrap() }).await;
        assert!(!Arc::ptr_eq(&first, &second));
    });
}
```

Lifecycle futures own the cached references. Completion, failure or cancellation releases them when the owning futures are dropped. Callers may retain an `Arc` beyond the scope; the value is destroyed when its last reference is released. New scopes start with fresh scoped instances, without taking an instance cached in the container's root lifecycle.

`forgetScopedInstances()` clears the current lifecycle's scoped cache while preserving other active lifecycles and ordinary singletons. Outside an execution scope it clears the root scoped instances. `forgetInstance(key)` still uses the original key, and `forgetInstances()` clears current scoped instances as well as the application's ordinary instance caches. A scoped `instance` override stays local to that lifecycle. Rebinding remains an application-level registration operation.

Container eviction does not clear a facade's separately cached root. Clear that facade cache when you need to re-resolve it within the same execution. Application-created Tokio tasks do not inherit ambient scopes; pass services explicitly or start the intended lifecycle around the task. Streaming bodies can retain already resolved services through owned `Arc` values. Streaming and termination callback wiring remain part of the HTTP delivery units.

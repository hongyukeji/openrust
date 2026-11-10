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
    app.singleton::<dyn Mailer, _>(|_| Ok(Arc::new(SmtpMailer)))?;
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

Use `make::<T>()` for a Rust type and `get::<T>(key)` for a named or explicit key. During boot, the configuration repository is bound under `config`, with `dyn openrust::contracts::config::Repository` as its typed alias. Every application owns its container; bindings and instances are not process-global.

Contextual bindings, extenders, resolution callbacks and rebinding are covered below. Deferred providers, dynamic constructor overrides and attribute projections remain unfinished; container unit 01-02 is not closed.

## Conditional Bindings and Instance State

`bindIf` and `singletonIf` register a typed factory only when the key has no binding, instance or alias. Their named forms are `bindIfNamed` and `singletonIfNamed`. The existence check and insertion are atomic; a rejected factory is not executed and is dropped outside the registry lock. Even an alias pointing to a missing service counts as an existing entry.

`isShared(key)` examines the original key without following aliases. `resolved(key)` follows aliases and returns true for an existing instance or a successfully resolved factory. Resolution history survives instance eviction. Supplying an instance does not execute or discard the original factory.

`forgetInstance(key)` removes the instance for that exact key. It does not follow aliases. A previously registered singleton factory can then construct a fresh instance. `forgetInstances()` clears all existing instances while preserving factories, aliases and resolution history. Instances without a backing factory become unbound; previously returned `Arc` values stay valid.

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{Container as _, BindingResolutionError}};

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonIfNamed::<u32, _>("counter", |_| Ok(Arc::new(7))).unwrap();
    app.bindIfNamed::<u32, _>("counter", |_| Ok(Arc::new(8))).unwrap();
    assert!(app.isShared("counter"));
    assert!(!app.resolved("counter")?);
    let original = app.get::<u32>("counter")?;
    app.instanceNamed("counter", Arc::new(99_u32)).unwrap();
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
    app.scoped::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
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

## Contextual binding and tagged services

`when::<Consumer>()?.needs::<Dependency>().give(factory)?` selects a dependency for the currently constructed consumer. `whenNamed(name)` and `whenMany(keys)` cover named and multiple consumers; `needsNamed::<T>("$parameter")` covers explicitly named constructor inputs. Factories resolve their dependencies explicitly because Rust does not provide PHP constructor reflection. The registered consumer identity takes the place of the reflected PHP class.

Contextual rules apply only to the nearest consumer. They take precedence over default instances, singletons and scoped caches, without replacing those caches or marking the dependency globally resolved. A contextual factory can resolve the same dependency again to use its default binding. Actual recursive factories still produce a circular-dependency error. Consumer aliases are normalized when `when` is called; dependency aliases are normalized when `give` is called, and later direct aliases are considered during lookup.

`give` accepts a typed factory. `giveService(key)` is the Rust overload of Laravel's service-name `give`: the implementation must be registered with the expected output contract. Use a factory to explicitly coerce `Arc<Concrete>` to `Arc<dyn Trait>`. `giveConfig(key, default)` reads the current `config` service when the dependency is resolved. Pass `None` for Laravel's omitted default. Missing keys use the default; an explicit JSON null stays null. Deserialization checks the declared Rust type and returns an error on mismatch; it does not silently coerce PHP scalar values.

`tag(keys, tags)` preserves registration order and duplicates. `tagged::<T>(tag)` resolves services lazily on iteration; each iteration resolves them anew using their registered lifetimes. An unknown tag yields an empty sequence. Existing sequences see services added before their next iteration; `len()` returns the count captured when the sequence was created, as Laravel's rewindable generator does. `giveTagged` collects the tagged services as `Vec<Arc<T>>`; an unresolved or mismatched entry propagates its error.

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::Container as _};
struct Report {
    channel: Arc<String>,
    port: Arc<u16>,
    services: Arc<Vec<Arc<String>>>,
}
fn main() {
    let container = Container::default();
    let config: Arc<dyn openrust::contracts::config::Repository> = Arc::new(openrust::config::Repository::new(Default::default()));
    container.instanceNamed("config", config.clone()).unwrap();
    config.set("reports.port", 8080.into()).unwrap();
    container.singleton::<String, _>(|_| Ok(Arc::new("default".into()))).unwrap();
    container.instanceNamed("local", Arc::new(String::from("local"))).unwrap();
    container.tag(["local"], ["reports"]);
    container.bind::<Report, _>(|c| Ok(Arc::new(Report {
        channel: c.make()?, port: c.get("$port")?, services: c.make()?,
    }))).unwrap();
    container.when::<Report>().unwrap()
        .needs::<String>().giveService("local").unwrap()
        .needsNamed::<u16>("$port").giveConfig("reports.port", None).unwrap()
        .needs::<Vec<Arc<String>>>().giveTagged("reports").unwrap();
    let report = container.make::<Report>().unwrap();
    assert_eq!(&*report.channel, "local");
    assert_eq!(*report.port, 8080);
    assert_eq!(report.services.len(), 1);
    assert_eq!(&*container.make::<String>().unwrap(), "default");
}
```

## Container resolution events

`beforeResolving`, `resolving` and `afterResolving` accept `None` for a global callback or `Some(ServiceKey)` for a selected identity. Aliases are normalized when registering the callback. The callback receives a `ResolutionEvent` and the current container, and returns `Result<(), BindingResolutionError>`. This follows the old implementation's event and ordered-selector design, adapted to the current synchronous factories.

The sequence is global before → selected before → construction → global resolving → selected resolving → global after → selected after. Before callbacks can register a missing service and execute on every lookup, including cache hits. Existing instances and shared cache hits skip post-construction callbacks. Each stage snapshots the global callbacks first, then reads selected callbacks after the globals have run: a global callback may register a selected callback for that same stage. A callback registered for a later stage can run during the current construction.

`event.key()` is the canonical identity fixed before the before callbacks. `event.value::<T>()` is `None` before construction and otherwise returns the existing `Arc<T>` when its declared output contract matches. Selected post-construction callbacks match the service key or its declared Rust output type. Rust cannot discover arbitrary PHP inheritance or interface relationships at runtime; register the intended output contract explicitly. Use interior mutability when a callback must configure a shared object. Dynamic constructor overrides and attribute projections are not part of this callback interface yet.

Shared objects are published before the post-construction callbacks, allowing those callbacks to resolve the same shared object. Other threads wait for callback completion instead of observing a partly configured object. If a post callback fails or panics, the already published shared object remains cached and the callback is not repeated on a cache hit; successful resolution history is not recorded. A failed transient has no cached object and is constructed again next time. Callback errors propagate through HTTP/Artisan's existing error handling. Unbounded same-service callback recursion returns a circular-dependency error; callback registration and execution do not hold the registry lock while running user code.

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::{Container, ServiceKey}, contracts::container::Container as _};
fn main() {
    let app = Container::default();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let before = trace.clone();
    app.beforeResolving(None, move |event, _| {
        assert!(event.value::<String>().is_none());
        before.lock().unwrap().push("before");
        Ok(())
    }).unwrap();
    app.singleton::<String, _>(|_| Ok(Arc::new("service".into()))).unwrap();
    let resolving = trace.clone();
    app.resolving(Some(ServiceKey::typed::<String>()), move |event, container| {
        assert_eq!(&*event.value::<String>().unwrap(), "service");
        assert!(container.resolved(event.key().clone())?);
        resolving.lock().unwrap().push("resolving");
        Ok(())
    }).unwrap();
    let after = trace.clone();
    app.afterResolving(None, move |_, _| {
        after.lock().unwrap().push("after");
        Ok(())
    }).unwrap();
    let first = app.make::<String>().unwrap();
    assert!(Arc::ptr_eq(&first, &app.make::<String>().unwrap()));
    assert_eq!(*trace.lock().unwrap(), ["before", "resolving", "after", "before"]);
}
```

## Extending bindings

`extend::<T, _>(key, |service, container| ...)` follows aliases and decorates the registered output contract. Return an `Arc<T>` containing the configured service or a wrapper implementing the same contract. The closure may resolve other services through its container argument. Contract mismatches are rejected before an incompatible factory or decorator runs.

If no instance is cached, `extend` saves the closure for future construction without eagerly making the service. These decorators run in registration order after construction and before resolving callbacks, including contextual constructions. A shared service caches the decorated value. A supplied `instance` bypasses the pending chain; forgetting it restores the original factory and that chain.

If an instance is already cached, `extend` immediately replaces that cached value. This closure is not added to the future chain. Existing callers retain their original `Arc`; the next lookup gets the replacement. As in Laravel, forgetting a cached instance discards an immediate-only decoration. `forgetExtenders(key)` removes future decorators, without undoing a decorated cached instance.

Construction and cached replacement run user code without the registry lock. A decorator registered while a constructor is pending is included before publication. Updates of the same cache slot serialize, and other readers wait until the update finishes; a decorator can read its own slot's old value. An in-flight update of a retired slot does not overwrite a newly rebound service. Recursive extension or dependency cycles return a circular-dependency error. An error or panic before publication leaves a shared constructor retryable; a failed cached update keeps its previous pointer. Mutations made inside a shared object are not rolled back.

Pending chains are application registrations. An immediate extension of an already cached scoped service changes only the current lifecycle's instance; the next lifecycle still uses its original factory and pending chain. A separately cached facade root is not automatically replaced: clear it or use an uncached facade before resolving again. Extenders and removed captures are released outside internal locks; retired decorator captures are released before publishing a new construction. Rebinding listeners and `refresh` are described below.

```rust
use std::sync::Arc;
use openrust::{container::{Container, ServiceKey}, contracts::container::Container as _};
trait Service: Send + Sync { fn name(&self) -> String; }
struct Base;
impl Service for Base { fn name(&self) -> String { "base".into() } }
struct Decorated(&'static str, Arc<dyn Service>);
impl Service for Decorated {
    fn name(&self) -> String { format!("{}:{}", self.0, self.1.name()) }
}
fn main() {
    let app = Container::default();
    let key = ServiceKey::typed::<dyn Service>();
    app.singleton::<dyn Service, _>(|_| Ok(Arc::new(Base))).unwrap();
    app.extend::<dyn Service, _>(key.clone(), |service, _| {
        Ok(Arc::new(Decorated("decorated", service)))
    }).unwrap();
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "decorated:base");
    app.extend::<dyn Service, _>(key.clone(), |service, _| {
        Ok(Arc::new(Decorated("cached", service)))
    }).unwrap();
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "cached:decorated:base");
    app.forgetInstance(key.clone());
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "decorated:base");
    app.forgetExtenders(key.clone()).unwrap();
    app.forgetInstance(key);
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "base");
}
```

## Rebinding and refreshing consumers

`rebinding::<T, _>(key, |container, service| ...)` follows aliases when registering a listener. If the service is already bound, registration resolves and returns `Some(Arc<T>)` without calling that listener; otherwise it returns `None`. The listener remains registered if this initial resolution fails.

Replacing an already resolved binding triggers its listeners. Supplying an instance triggers listeners only if that raw key was already bound before the replacement. First bindings, rejected conditional bindings and instance eviction do not notify. A future decorator added to an already resolved transient or an evicted shared service rebuilds it for listeners; a cached decoration notifies with the replacement value. Each event holds its own committed value and ordered callback snapshot, so concurrent updates do not deliver a later value for an earlier event. A listener registered during dispatch participates in later events.

Callbacks run after the new binding or cache is committed. Their errors and panics propagate without rolling that state back. Later listeners in the failed event are skipped. Recursive same-service notification returns a cycle error after the nested change has committed. Binding registration methods therefore return `Result`; propagate errors with `?`. `instance` and `instanceNamed` return the supplied `Arc` on success.

`refresh::<T, _, _>(key, target, setter)` is the Rust form of Laravel's target/method refresh. It accepts an explicit `Arc` target and setter instead of a PHP method-name string. Registration returns the current service without initially calling the setter; later rebindings call it. Callbacks use explicit output contracts, execute outside registry locks, and preserve application and scoped-instance isolation. A facade swap updates its facade cache before rebinding the container; a listener failure leaves both committed values in place.

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::Container, contracts::container::{BindingResolutionError, Container as _}};
fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.instanceNamed("service", Arc::new(String::from("first")))?;
    let target = Arc::new(Mutex::new(String::new()));
    let current = app.refresh::<String, _, _>("service", target.clone(), |target, value| {
        *target.lock().unwrap() = (*value).clone();
        Ok(())
    })?.unwrap();
    assert_eq!(&*current, "first");
    assert!(target.lock().unwrap().is_empty());
    app.instanceNamed("service", Arc::new(String::from("second")))?;
    assert_eq!(&*target.lock().unwrap(), "second");
    Ok(())
}
```

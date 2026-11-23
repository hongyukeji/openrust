# Service Container

## Binding

Register services in a provider's `register` method through `app.container()`. `bind::<T, _>` invokes its factory on every resolution, `singleton::<T, _>` shares the first successful result, and `instance::<T>` supplies an existing `Arc<T>`. Factories receive the same container so they can resolve dependencies explicitly.

Rust does not provide PHP constructor reflection. Bind a trait as `dyn Trait` and return a concrete implementation from its factory. Rust verifies the interface implementation at compilation, while container failures are returned as `BindingResolutionError`. A type without a binding or compiled constructor metadata is not implicitly constructed.

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

Use `bind_named::<T, _>(name, factory)`, `singleton_named::<T, _>(name, factory)` or `instance_named::<T>(name, instance)` for named entries. These are the named forms of the same operations: Rust cannot overload a method by its argument count. `get::<T>(name)` resolves a named entry and checks its output type before invoking the factory. Typed and named entries remain distinct even when their displayed names match.

`alias(abstract, alias)` accepts a `ServiceKey::typed::<T>()`, `ServiceKey::named(name)` or a string. Aliases may point to other aliases or to entries registered later. `is_alias` tests the original key; `get_alias` follows the chain; `has` checks a binding or alias without constructing it. Names are exact and case-sensitive. Binding or supplying an instance at an alias key replaces that alias. Rebinding the target affects subsequent resolutions; already returned `Arc` values remain valid.

A direct self-alias is rejected. Alias cycles and factory dependency cycles return errors. A failed or panicking singleton constructor does not cache a value; a later call may retry. Concurrent resolutions share the first successful singleton. Container locks are released before invoking user factories.

## Resolving

Use `make::<T>()` for a Rust type and `get::<T>(key)` for a named or explicit key. During boot, the configuration repository is bound under `config`, with `dyn openrust::contracts::config::Repository` as its typed alias. Every application owns its container; bindings and instances are not process-global.

Use `make_with` to supply explicit construction arguments. Use `make_async` to await dependencies provided by a deferred provider or an asynchronous factory. See [Parameter Overrides](#parameter-overrides) and [Service Providers](./providers.md).

## Conditional Bindings and Instance State

`bind_if` and `singleton_if` register a typed factory only when the key has no binding, instance or alias. Their named forms are `bind_if_named` and `singleton_if_named`. The existence check and insertion are atomic; a rejected factory is not executed and is dropped outside the registry lock. Even an alias pointing to a missing service counts as an existing entry.

`is_shared(key)` examines the original key without following aliases. `resolved(key)` follows aliases and returns true for an existing instance or a successfully resolved factory. Resolution history survives instance eviction. Supplying an instance does not execute or discard the original factory.

`forget_instance(key)` removes the instance for that exact key. It does not follow aliases. A previously registered singleton factory can then construct a fresh instance. `forget_instances()` clears all existing instances while preserving factories, aliases and resolution history. Instances without a backing factory become unbound; previously returned `Arc` values stay valid.

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{Container as _, BindingResolutionError}};

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singleton_if_named::<u32, _>("counter", |_| Ok(Arc::new(7))).unwrap();
    app.bind_if_named::<u32, _>("counter", |_| Ok(Arc::new(8))).unwrap();
    assert!(app.is_shared("counter"));
    assert!(!app.resolved("counter")?);
    let original = app.get::<u32>("counter")?;
    app.instance_named("counter", Arc::new(99_u32)).unwrap();
    assert_eq!(*app.get::<u32>("counter")?, 99);
    app.forget_instance("counter");
    let rebuilt = app.get::<u32>("counter")?;
    assert_eq!(*rebuilt, 7);
    assert!(!Arc::ptr_eq(&original, &rebuilt));
    Ok(())
}
```

These operations do not clear a facade's own cached root; use its cache-clearing API when required. Scoped service eviction is described below.

## Scoped Bindings

`scoped::<T, _>` shares a service within one execution lifecycle. Its named form is `scoped_named::<T, _>(name, factory)`. Use `scoped_if` or `scoped_if_named` to register only when the original key is unbound. HTTP handler execution and each Artisan dispatch automatically enter a lifecycle. Ordinary `singleton` instances remain shared by the application.

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

`forget_scoped_instances()` clears the current lifecycle's scoped cache while preserving other active lifecycles and ordinary singletons. Outside an execution scope it clears the root scoped instances. `forget_instance(key)` still uses the original key, and `forget_instances()` clears current scoped instances as well as the application's ordinary instance caches. A scoped `instance` override stays local to that lifecycle. Rebinding remains an application-level registration operation.

Container eviction does not clear a facade's separately cached root. Clear that facade cache when you need to re-resolve it within the same execution. Application-created Tokio tasks do not inherit ambient scopes; pass services explicitly or start the intended lifecycle around the task. Streaming bodies can retain already resolved services through owned `Arc` values.

## Contextual binding and tagged services

`when::<Consumer>()?.needs::<Dependency>().give(factory)?` selects a dependency for the currently constructed consumer. `when_named(name)` and `when_many(keys)` cover named and multiple consumers; `needs_named::<T>("$parameter")` covers explicitly named constructor inputs. Factories resolve their dependencies explicitly because Rust does not provide PHP constructor reflection. The registered consumer identity takes the place of the reflected PHP class.

Contextual rules apply only to the nearest consumer. They take precedence over default instances, singletons and scoped caches, without replacing those caches or marking the dependency globally resolved. A contextual factory can resolve the same dependency again to use its default binding. Actual recursive factories still produce a circular-dependency error. Consumer aliases are normalized when `when` is called; dependency aliases are normalized when `give` is called, and later direct aliases are considered during lookup.

`give` accepts a typed factory. `give_service(key)` is the Rust overload of service-name `give`: the implementation must be registered with the expected output contract. Use a factory to explicitly coerce `Arc<Concrete>` to `Arc<dyn Trait>`. `give_config(key, default)` reads the current `config` service when the dependency is resolved. Pass `None` for omitted default. Missing keys use the default; an explicit JSON null stays null. Deserialization checks the declared Rust type and returns an error on mismatch; it does not silently coerce PHP scalar values.

`tag(keys, tags)` preserves registration order and duplicates. `tagged::<T>(tag)` resolves services lazily on iteration; each iteration resolves them anew using their registered lifetimes. An unknown tag yields an empty sequence. Existing sequences see services added before their next iteration; `len()` returns the count captured when the sequence was created, as rewindable generator does. `give_tagged` collects the tagged services as `Vec<Arc<T>>`; an unresolved or mismatched entry propagates its error.

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
    container.instance_named("config", config.clone()).unwrap();
    config.set("reports.port", 8080.into()).unwrap();
    container.singleton::<String, _>(|_| Ok(Arc::new("default".into()))).unwrap();
    container.instance_named("local", Arc::new(String::from("local"))).unwrap();
    container.tag(["local"], ["reports"]);
    container.bind::<Report, _>(|c| Ok(Arc::new(Report {
        channel: c.make()?, port: c.get("$port")?, services: c.make()?,
    }))).unwrap();
    container.when::<Report>().unwrap()
        .needs::<String>().give_service("local").unwrap()
        .needs_named::<u16>("$port").give_config("reports.port", None).unwrap()
        .needs::<Vec<Arc<String>>>().give_tagged("reports").unwrap();
    let report = container.make::<Report>().unwrap();
    assert_eq!(&*report.channel, "local");
    assert_eq!(*report.port, 8080);
    assert_eq!(report.services.len(), 1);
    assert_eq!(&*container.make::<String>().unwrap(), "default");
}
```

## Container resolution events

`before_resolving`, `resolving` and `after_resolving` accept `None` for a global callback or `Some(ServiceKey)` for a selected identity. Aliases are normalized when registering the callback. The callback receives a `ResolutionEvent` and the current container, and returns `Result<(), BindingResolutionError>`.

The sequence is global before → selected before → construction → global resolving → selected resolving → global after → selected after. Before callbacks can register a missing service and execute on every lookup, including cache hits. Existing instances and shared cache hits skip post-construction callbacks. Each stage snapshots the global callbacks first, then reads selected callbacks after the globals have run: a global callback may register a selected callback for that same stage. A callback registered for a later stage can run during the current construction.

`event.key()` is the canonical identity fixed before the before callbacks. A before callback reads explicit construction arguments through `event.parameters()`. Resolving and after-resolving callbacks read the constructed object through `event.value::<T>()`, including its declared trait views. Use interior mutability when a callback must configure a shared object. [Resolving Events for Interface Views](#resolving-events-for-interface-views)

Shared objects are published before the post-construction callbacks, allowing those callbacks to resolve the same shared object. Other threads wait for callback completion instead of observing a partly configured object. If a post callback fails or panics, the already published shared object remains cached and the callback is not repeated on a cache hit; successful resolution history is not recorded. A failed transient has no cached object and is constructed again next time. Callback errors propagate through HTTP/Artisan's existing error handling. Unbounded same-service callback recursion returns a circular-dependency error; callback registration and execution do not hold the registry lock while running user code.

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::{Container, ServiceKey}, contracts::container::Container as _};
fn main() {
    let app = Container::default();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let before = trace.clone();
    app.before_resolving(None, move |event, _| {
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
    app.after_resolving(None, move |_, _| {
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

If an instance is already cached, `extend` immediately replaces that cached value. This closure is not added to the future chain. Existing callers retain their original `Arc`; the next lookup gets the replacement. As in the framework, forgetting a cached instance discards an immediate-only decoration. `forget_extenders(key)` removes future decorators, without undoing a decorated cached instance.

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
    app.forget_instance(key.clone());
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "decorated:base");
    app.forget_extenders(key.clone()).unwrap();
    app.forget_instance(key);
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "base");
}
```

## Rebinding and refreshing consumers

`rebinding::<T, _>(key, |container, service| ...)` follows aliases when registering a listener. If the service is already bound, registration resolves and returns `Some(Arc<T>)` without calling that listener; otherwise it returns `None`. The listener remains registered if this initial resolution fails.

Replacing an already resolved binding triggers its listeners. Supplying an instance triggers listeners only if that raw key was already bound before the replacement. First bindings, rejected conditional bindings and instance eviction do not notify. A future decorator added to an already resolved transient or an evicted shared service rebuilds it for listeners; a cached decoration notifies with the replacement value. Each event holds its own committed value and ordered callback snapshot, so concurrent updates do not deliver a later value for an earlier event. A listener registered during dispatch participates in later events.

Callbacks run after the new binding or cache is committed. Their errors and panics propagate without rolling that state back. Later listeners in the failed event are skipped. Recursive same-service notification returns a cycle error after the nested change has committed. Binding registration methods therefore return `Result`; propagate errors with `?`. `instance` and `instance_named` return the supplied `Arc` on success.

`refresh::<T, _, _>(key, target, setter)` is the Rust form of target/method refresh. It accepts an explicit `Arc` target and setter instead of a PHP method-name string. Registration returns the current service without initially calling the setter; later rebindings call it. Callbacks use explicit output contracts, execute outside registry locks, and preserve application and scoped-instance isolation. A facade swap updates its facade cache before rebinding the container; a listener failure leaves both committed values in place.

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::Container, contracts::container::{BindingResolutionError, Container as _}};
fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.instance_named("service", Arc::new(String::from("first")))?;
    let target = Arc::new(Mutex::new(String::new()));
    let current = app.refresh::<String, _, _>("service", target.clone(), |target, value| {
        *target.lock().unwrap() = (*value).clone();
        Ok(())
    })?.unwrap();
    assert_eq!(&*current, "first");
    assert!(target.lock().unwrap().is_empty());
    app.instance_named("service", Arc::new(String::from("second")))?;
    assert_eq!(&*target.lock().unwrap(), "second");
    Ok(())
}
```

## Asynchronous Factories

Use an asynchronous factory when constructing a service requires I/O or a deferred dependency. These methods use the existing binding registry and lifetimes; they do not create a second container. PHP closure execution becomes an explicitly awaited Rust future. Existing synchronous methods remain available.

| Lifetime | Typed factory | Named factory |
| --- | --- | --- |
| Transient | `bind_async` | `bind_async_named` |
| Singleton | `singleton_async` | `singleton_async_named` |
| Scoped | `scoped_async` | `scoped_async_named` |

Conditional variants put `If` before `Async`: for example, `bind_if_async`, `singleton_if_async_named`, and `scoped_if_async`. Await registration, including rebinding notifications. A rejected conditional registration never invokes its factory. A factory receives the current container and returns `Box::pin(async move { ... })` with `Result<Arc<T>, BindingResolutionError>`; the public alias is `openrust::container::FactoryFuture`.

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{BindingResolutionError, Container as _}};
struct Client;
struct Worker(Arc<Client>);
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singleton_async::<Client, _>(|_| Box::pin(async {
        tokio::task::yield_now().await;
        Ok(Arc::new(Client))
    })).await?;
    app.scoped_async::<Worker, _>(|container| Box::pin(async move {
        Ok(Arc::new(Worker(container.make_async::<Client>().await?)))
    })).await?;
    app.scope(async {
        let a = app.make_async::<Worker>().await?;
        let b = app.make_async::<Worker>().await?;
        assert!(Arc::ptr_eq(&a, &b));
        assert!(Arc::ptr_eq(&a.0, &app.make_async::<Client>().await?));
        Ok::<(), BindingResolutionError>(())
    }).await?;
    Ok(())
}
```

Inside a factory, `make_async` and `get_async` automatically load a dependency's deferred provider and wait for readiness. The same consumer context, aliases, output contracts, decorators, and resolution callbacks apply to synchronous and asynchronous bindings. A synchronous lookup of an uncached asynchronous factory returns `AsyncResolutionRequired`; a ready shared cache is readable synchronously.

Use contextual `give_async`, `give_service_async`, `give_config_async`, or `give_tagged_async` when the selected value needs asynchronous resolution. `tagged(...).collect_async().await` resolves a collection; `iter().next_async().await` resolves one entry at a time. Cancelling the latter leaves that entry available for the next call. `rebinding_async`, `refresh_async`, and `extend_async` await required construction and notification, while their callbacks and decorators remain synchronous. Perform awaited initialization in a factory, not a synchronous callback.

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::{Container, ServiceKey}, contracts::container::{BindingResolutionError, Container as _}};
struct Worker(Arc<String>);
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.bind_async_named::<String, _>("message", |_| Box::pin(async {
        Ok(Arc::new(String::from("first")))
    })).await?;
    app.when::<Worker>()?.needs::<String>().give_service_async("message")?;
    app.bind_async::<Worker, _>(|container| Box::pin(async move {
        Ok(Arc::new(Worker(container.make_async::<String>().await?)))
    })).await?;
    app.tag([ServiceKey::typed::<Worker>()], ["workers"]);
    let workers = app.tagged::<Worker>("workers").collect_async().await?;
    assert_eq!(&*workers[0].0, "first");
    let target = Arc::new(Mutex::new(String::new()));
    app.refresh_async::<String, _, _>("message", target.clone(), |target, value| {
        *target.lock().unwrap() = (*value).clone();
        Ok(())
    }).await?;
    app.bind_async_named::<String, _>("message", |_| Box::pin(async {
        Ok(Arc::new(String::from("second")))
    })).await?;
    assert_eq!(&*target.lock().unwrap(), "second");
    Ok(())
}
```

Concurrent shared lookups wait without blocking the executor and share one construction attempt. Failed or cancelled construction wakes its existing waiters with an error; a later lookup can retry. Dependency cycles return an error. A value already published before a resolving callback fails remains cached, following the existing container behavior.

The framework restores construction context on each poll, so a suspended factory cannot leak its consumer into another future. Request and command completion closes their scope and drains its caches even when a future is retained. Further container resolution through that closed scope returns `ScopeClosed`; a facade future moved into a different application execution returns `FacadeNotSet`. Dropping pending work restores its live origin for cleanup; a closed origin has no ambient facade. Caller-owned futures and service references remain the caller's responsibility and are released when dropped. These explicit async errors and lifecycle guards adapt PHP's sequential mechanism to Rust's concurrent runtime.

## Parameter Overrides

Use `Application::make_with::<T>(parameters).await` to supply explicit construction arguments. Container callers use synchronous `make_with` or `make_with_async`; named keys use `make_with_named` and `make_with_async_named`. The application provides `make_with_named(...).await` for named keys.

An empty set follows normal resolution and may reuse a shared instance. A nonempty set selects the registered factory, bypasses supplied instances and shared caches, and constructs a separate value. It applies contextual selection, decorators and resolving callbacks without replacing the ordinary shared value or recording ordinary resolved history. Supplying parameters to an instance-only service with no factory returns `NotBound`.

PHP passes its factory an argument array. Rust factories retain their explicit container argument and read the current immutable snapshot through `get_last_parameter_override()`. `ResolutionEvent::parameters()` exposes the snapshot to before-resolving callbacks. Ordinary nested lookups receive an empty set; forward the snapshot explicitly when a dependency should use it. Each async poll restores its own frame, including when an ordinary pending lookup resumes inside another parameterized construction.

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ParameterKey, ParameterOverrides, ParameterValue, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
fn arguments(number: u32) -> ParameterOverrides {
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("number"), ParameterValue::new(number));
    parameters
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singleton_async::<u32, _>(|container| Box::pin(async move {
        tokio::task::yield_now().await;
        let value = container.get_last_parameter_override()
            .get(&ParameterKey::name("number"))
            .map(|value| value.downcast::<u32>()).transpose()?;
        Ok(Arc::new(value.map_or(0, |value| *value)))
    })).await?;
    assert_eq!(*app.make_with_async::<u32>(arguments(7)).await?, 7);
    assert!(!app.resolved(ServiceKey::typed::<u32>())?);
    let normal = app.make_async::<u32>().await?;
    assert_eq!(*normal, 0);
    assert_eq!(*app.make_with_async::<u32>(arguments(9)).await?, 9);
    assert!(Arc::ptr_eq(&normal, &app.make_async::<u32>().await?));
    Ok(())
}
```

`ParameterOverrides` preserves insertion order. Replacing a key retains its position and returns the previous value; cloning shares immutable data and later mutation copies the collection. `ParameterKey::Name` and `Index` are explicit identities: numeric strings are not silently converted to integer keys. `ParameterValue::null()` differs from a missing key. Values need `Send + Sync + 'static`, but need not be cloned or serialized; `from_arc` preserves an existing object's identity. To store an `Arc<dyn Trait>`, wrap it with `new` and read that same `Arc<dyn Trait>` type.

`downcast` requires the declared Rust type; a wrong type or null returns `ParameterTypeMismatch` without exposing payload contents. This carrier performs no PHP scalar coercion. Errors, panics and cancellation restore the previous frame. Pending factory cleanup sees its own arguments; retired argument payloads are dropped with an empty argument frame so they cannot read another call's parameters. Caller-retained snapshots and service references remain owned by the caller.

## Automatic Constructor Injection

Rust has no PHP runtime constructor reflection. Register `ClassDescriptor` metadata in a provider's `register` hook through `app.container().register_class(...)`. A descriptor declares named parameter types and a compiled constructor, replacing reflection's inspection and dynamic invocation. Once registered, normal `make` / `make_async` automatically resolve its dependencies; no service factory binding is required. Types with neither metadata nor a binding return `NotBound`.

`ClassDescriptor::concrete::<T, _>` accepts a synchronous constructor; `concrete_async` accepts an awaited constructor. Synchronous lookup cannot initialize an async constructor or dependency and returns the existing async-required error. The async path automatically loads deferred dependency providers. Metadata is application-local and immutable: registering the same descriptor clone is idempotent, while a different definition for the same type is rejected. Metadata alone does not make `bound` or `is_shared` true. Ordinary successful resolution records history, while explicit nonempty parameter overrides do not.

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ClassDescriptor, ConstructorParam, ParameterKey, ParameterOverrides, ParameterValue},
    contracts::container::{BindingResolutionError, Container as _},
};
struct Client;
struct Service { client: Arc<Client>, name: String }
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.register_class(ClassDescriptor::concrete_async::<Client, _>(vec![], |_| Box::pin(async {
        tokio::task::yield_now().await;
        Ok(Arc::new(Client))
    }))?)?;
    app.register_class(ClassDescriptor::concrete::<Service, _>(vec![
        ConstructorParam::class::<Client>("client"),
        ConstructorParam::scalar::<String>("name")
            .with_default(|| Ok(ParameterValue::new(String::from("default")))),
    ], |args| Ok(Arc::new(Service {
        client: args.service(0)?, name: (*args.get::<String>(1)?).clone(),
    })))?)?;
    assert!(!app.bound::<Service>());
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("name"), ParameterValue::new(String::from("custom")));
    assert_eq!(app.make_with_async::<Service>(parameters).await?.name, "custom");
    app.singleton_async::<Service, _>(|container| Box::pin(async move {
        container.build_async::<Service>().await
    })).await?;
    let service = app.make_async::<Service>().await?;
    assert_eq!(service.name, "default");
    let cached = app.make_async::<Service>().await?;
    assert!(Arc::ptr_eq(&service.client, &cached.client));
    assert!(Arc::ptr_eq(&service, &cached));
    Ok(())
}
```

`ConstructorParam::scalar::<T>("name")` selects a value of the declared Rust type. `class::<T>("name")` selects an `Arc<T>`, including `dyn Trait` contracts. `ResolvedArgs::get` reads scalar slots, `service` returns the original service `Arc`, and `value` exposes the typed carrier. Slots follow declaration order; duplicate or empty names are rejected before registration.

A matching explicit named override wins. The first [contextual attribute](#contextual-attributes) is evaluated next. A null attribute result falls back to ordinary resolution. A primitive then uses its `$name` contextual binding, `with_default`, or a null value if `nullable` is enabled. A class default applies when that class has no explicit binding or direct contextual binding. Otherwise the container resolves the class. A nullable class without a default still attempts dependency resolution. Use `nullable().with_default(|| Ok(ParameterValue::null()))` for an optional default. Read nullable slots through `optional` or `optional_service`.

Dependencies and defaults are evaluated in declaration order, then the selected arguments are type-checked before invoking the constructor. Default functions are lazy and evaluated afresh when selected. Constructor arguments retain strict Rust types; this adapter does not implement PHP weak scalar coercion. Nested ordinary dependency resolution starts with empty overrides. The normal context, extenders, resolution callbacks and readiness protection remain in effect.

`build::<T>()` / `build_async::<T>().await` directly invoke the compiled constructor, bypassing the target binding, cache, extenders, container resolving callbacks and resolved history. Parameter and class attribute callbacks still run. Dependencies use ordinary container resolution. Use `singleton(|container| container.build::<T>())`, `scoped`, or their async equivalents to give a described class a shared lifetime. `build` uses the active parameter snapshot, so a parameterized factory can forward arguments within the current resolution.

Register compiled adapters for [method invocation](#method-invocation), [SelfBuilding](#self-building-classes) and [interface views](#compiled-interface-views) as needed. Each declaration supplies the type information the container uses in place of PHP runtime reflection.

## Variadic Injection

Mark the final `ConstructorParam` as `variadic()`. It cannot have a default, and only one variadic declaration is permitted. `ResolvedArgs::values(index)` reads its ordered items, including an empty list; `services::<T>(index)` returns service references. `len()` counts declared slots. Ordinary `value` / `get` / `service` access rejects a variadic slot with `InvalidArgumentShape`.

A named override supplies exactly one item. It is not spread when that item is itself a vector. Integer-indexed overrides do not replace the named slot. Each selected item must match the declared Rust type; `nullable()` also permits explicit null items. Neither an omitted primitive variadic nor an unresolved class variadic injects a null: they produce an empty list. A constructible class without a contextual list contributes one normally resolved instance.

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ClassDescriptor, ConstructorParam},
    contracts::container::{BindingResolutionError, Container as _},
};
trait Filter: Send + Sync { fn name(&self) -> &str; }
struct Named(&'static str);
impl Filter for Named { fn name(&self) -> &str { self.0 } }
struct Pipeline(Vec<Arc<dyn Filter>>);
fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.bind_named::<dyn Filter, _>("first", |_| Ok(Arc::new(Named("first"))))?;
    app.bind_named::<dyn Filter, _>("second", |_| Ok(Arc::new(Named("second"))))?;
    app.register_class(ClassDescriptor::concrete::<Pipeline, _>(
        vec![ConstructorParam::class::<dyn Filter>("filters").variadic()],
        |args| Ok(Arc::new(Pipeline(args.services(0)?))),
    )?)?;
    app.when::<Pipeline>()?.needs::<dyn Filter>()
        .give_services(["first", "second", "first"])?;
    let pipeline = app.make::<Pipeline>()?;
    let names: Vec<_> = pipeline.0.iter().map(|filter| filter.name()).collect();
    assert_eq!(names, ["first", "second", "first"]);
    Ok(())
}
```

Use `needs::<T>().give_services(keys)` for `give(array)` class-service list. Keys resolve in order, including duplicates, within the original consumer context. Registration is lazy, normalizes dependency aliases, and uses an immutable key snapshot for an in-flight construction. The asynchronous constructor channel waits for deferred or async services without requiring a separate registration method.

Tagged or computed collection factories use the existing typed collection contract: `needs::<Vec<Arc<T>>>().give_tagged(tag)` or `give_tagged_async(tag)`. The variadic constructor can consume that collection. A direct contextual binding for `T` takes precedence over the collection adapter. Collection factories execute as their own resolution, so items do not inherit the consuming class's contextual bindings; direct `give_services` arrays preserve that class context. Collection callbacks use the `Vec<Arc<T>>` identity, reflecting the Rust list contract.

For primitives, bind `$name` to `Vec<Arc<T>>` or `Vec<ParameterValue>`. For example, `needs_named::<Vec<Arc<String>>>("$names").give_config("app.names", None)` reads the current configuration array. The framework enables Serde's `rc` feature for these typed lists. Shared item references support values without `Clone`; heterogeneous carriers preserve null and are checked individually. These ownership representations add argument wrappers and reference counting.

Primitive contextual closures execute inline in the constructor context, as in primitive resolver. They keep the constructor's parameter snapshot and do not create an additional service event/cache/history for `$name`. Normal dependency lookups inside the closure still follow ordinary container behavior. Async primitive closures can await initialization in that same context.

A class variadic catches missing bindings and unresolved primitive dependency errors and returns an empty list for the whole argument, including a partially resolved service array. Business failures, type mismatches, dependency/alias cycles, async-required errors, provider failures and closed lifecycles propagate. Primitive variadic errors also propagate. Cancellation releases partial values under the originating context. Shared services and any values explicitly retained by application code keep their usual ownership.

## Method Invocation

Use `call(target, parameters).await` on a Container or Application. Rust has no PHP runtime reflection: `Callback` supplies closure parameter metadata, while `register_method` and `register_static_method` register compiled method adapters. The callback can borrow data and return a generic future. Registered method results require `Send + 'static`, but do not require `Sync`; anonymous callbacks may return borrowed results.

```rust
use openrust::{
    container::{Callback, ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue},
    contracts::container::Container as _,
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    let signature = [ConstructorParam::scalar::<u32>("number")];
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("number"), ParameterValue::new(7u32));
    let mut result = String::new();
    let result = &mut result;
    let value = container.call(Callback::new(&signature, move |args| async move {
        result.push_str(&args.resolved.get::<u32>(0)?.to_string());
        Ok(result.as_str())
    }), parameters).await.unwrap();
    assert_eq!(value, "7");
}
```

BoundMethod has its own parameter policy. A matching name wins, then `ParameterKey::class::<T>()`, then class injection. Each explicit key is consumed once. A class default applies when that class is not bound, even if a contextual-only entry exists. Scalar defaults apply; nullable alone does not supply a missing value, and scalar method parameters do not use `$name` contextual bindings. Integer keys and other unconsumed inputs are retained in insertion order in `CallArguments.remaining`; they do not replace missing named parameters. A final variadic slot receives those remaining values. An explicit array remains one value; computed class collections are spread, constructor `give_services` arrays are not, and missing class dependencies propagate errors.

```rust
use openrust::{
    container::{ConstructorParam, Container, MethodCall, ParameterOverrides, StaticCall},
    contracts::container::Container as _,
};
use std::sync::Arc;

struct Worker;
struct Client(&'static str);

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    container.instance(Arc::new(Worker)).unwrap();
    container.instance(Arc::new(Client("ready"))).unwrap();
    container.register_method::<Worker, String, _>("run",
        vec![ConstructorParam::class::<Client>("client")],
        |_, args, _| Box::pin(async move {
            Ok(args.resolved.service::<Client>(0)?.0.to_owned())
        }),
    ).unwrap();
    let value: String = container.call(
        MethodCall::<Worker>::class("run"), ParameterOverrides::new(),
    ).await.unwrap();
    assert_eq!(value, "ready");
    container.bind_method::<Worker, String, _>("run", |_, _| {
        Box::pin(async { Ok("replacement".to_owned()) })
    }).unwrap();
    assert!(container.has_method_binding::<Worker>("run"));
    let value: String = container.call(
        MethodCall::on(Arc::new(Worker), "run"), ParameterOverrides::new(),
    ).await.unwrap();
    assert_eq!(value, "replacement");
    container.register_static_method::<Worker, u32, _>("version", vec![], |_, _| {
        Box::pin(async { Ok(1) })
    }).unwrap();
    container.bind_static_method::<Worker, u32, _>("version", |_, _| {
        Box::pin(async { Ok(2) })
    }).unwrap();
    assert_eq!(container.call::<u32, _>(StaticCall::<Worker>::qualified("version"),
        ParameterOverrides::new()).await.unwrap(), 1);
    assert_eq!(container.call::<u32, _>(StaticCall::<Worker>::array("version"),
        ParameterOverrides::new()).await.unwrap(), 2);
}
```

`MethodCall::on(receiver, method)` represents an object array callable. `class(method)` represents `Class@method`: it resolves the receiver before looking up an override. `default_method(method)` and `invokable()` represent the class default and declared `__invoke`; a binding alone does not establish that declaration. Empty and "0" default method names use the invokable path. Registered declarations use ASCII case-insensitive lookup; method bindings use the exact caller spelling.

`bind_method` replaces the declared body and skips its parameter injection and validation. `has_method_binding` checks the override; `call_method_binding` invokes it directly without adding a class context. Static adapters use `bind_static_method` and `call_static_method_binding`. `StaticCall::qualified` corresponds to a `Class::method` string and ignores method bindings, while `StaticCall::array` corresponds to a class-array callable and consults them. Invalid receiver forms or result contracts return typed errors.

An actual callable object or static method establishes a class context only if that class is absent from the logical build stack. `Class@method` retains the parent context, as in the framework BoundMethod.callClass. Dependencies still enter their own normal construction contexts. Contexts are restored on every async poll and cancellation; separate applications and concurrent calls remain isolated. Application entry methods also establish the application Facade context. Running calls keep their selected override, original parameter ownership and receiver alive until completion or cancellation.

The compiled type `C` replaces PHP class-name reflection; Rust trait aliases and concrete classes do not gain automatic method projection. Callback adapters return `Result<R, BindingResolutionError>` for injection failures; `R` may itself be a domain result. Stored callbacks and the callable dispatch future are boxed to support asynchronous borrowing on Rust 1.94.

## Lazy Closures

`factory::<T>()` returns a closure that calls `make::<T>()` each time. `factory_named::<T>(key)` keeps the original owned ServiceKey and performs live alias lookup. Creation does not check or resolve the service. Later registrations, replacements and instance eviction are visible; singleton/scoped behavior is exactly the ordinary resolver behavior. The asynchronous container overloads are `factory_async` and `factory_async_named`. Application exposes asynchronous `factory` and `factory_named`, establishing its Facade context for each resolution.

`wrap(Callback::reusable(signature, callback), parameters)` returns a reusable asynchronous closure around `call`. The input parameter map is retained as a copy-on-write snapshot, while dependencies and defaults are selected again on every invocation. The same callback state is shared; neither the callback nor its results are cloned or cached. Errors and cancellation do not consume the wrapper. Scope and class context follow execution, rather than being frozen when the wrapper is created.

```rust
use openrust::{
    container::{Callback, ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue},
    contracts::container::Container as _,
};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    let factory = container.factory::<u32>();
    container.instance(Arc::new(3u32)).unwrap();
    assert_eq!(*factory().unwrap(), 3);
    container.instance(Arc::new(9u32)).unwrap();
    assert_eq!(*factory().unwrap(), 9);

    let signature = [ConstructorParam::class::<u32>("number"),
        ConstructorParam::scalar::<u32>("increment")];
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("increment"), ParameterValue::new(2u32));
    let wrapped = container.wrap(Callback::reusable(&signature, |args| async move {
        Ok(*args.resolved.service::<u32>(0)? + *args.resolved.get::<u32>(1)?)
    }), parameters.clone());
    parameters.insert(ParameterKey::name("increment"), ParameterValue::new(7u32));
    assert_eq!(wrapped().await.unwrap(), 11);
    container.instance(Arc::new(5u32)).unwrap();
    assert_eq!(wrapped().await.unwrap(), 7);

    let async_factory = container.factory_async_named::<u32>(
        openrust::container::ServiceKey::typed::<u32>(),
    );
    assert_eq!(*async_factory().await.unwrap(), 5);
}
```

`Callback::reusable` gives Rust 1.94 closure inference the repeatable `Fn` contract. `Callback::new` still supports `FnOnce` for ordinary calls. A wrapped callback requires `Send + Sync`; use synchronized shared state for mutation. Wrapper closures borrow their Container/Application and cannot outlive it. Each invocation returns its own Send future and may return externally borrowed data. Concrete returned closures avoid a boxed wrapper; shared callback ownership uses an Arc, and asynchronous dispatch retains the boxed-future adaptation described above.

## Binding Directory and Flush

`get_bindings()` returns an owned insertion-ordered `Bindings` snapshot. `get(&ServiceKey)` performs exact lookup, while `iter`, `keys` and owned iteration preserve registration order. Replacing a binding keeps its position. Each `BindingDefinition` exposes the original `concrete` factory and declared `shared` flag; an explicit instance override does not change that flag. Alias-only keys, instance-only keys, contextual bindings and automatic class metadata are excluded. Reading or modifying the snapshot does not resolve services or mutate the container.

The retained `FactoryHandle` corresponds to invoking the concrete closure from binding array. `call::<T>(&container, parameters)` and `call_async::<T>(...)` use the supplied container and explicit parameter frame, without adding the target class context. They bypass the target cache, decorators, events and resolution history; dependency lookups still use ordinary behavior with fresh parameter frames. A raw shared factory therefore constructs a fresh result every time. Wrong output types fail before executing user code, and synchronous invocation rejects an asynchronous factory even if normal resolution has already cached its result. `same_factory` compares retained recipe identity.

```rust
use openrust::{
    container::{Container, ParameterOverrides, ServiceKey},
    contracts::container::Container as _,
};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    container.singleton::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    container.extend::<u32, _>(ServiceKey::typed::<u32>(), |value, _| {
        Ok(Arc::new(*value + 10))
    }).unwrap();
    let definitions = container.get_bindings();
    let definition = definitions.get(&ServiceKey::typed::<u32>()).unwrap();
    assert!(definition.shared);
    assert_eq!(*definition.concrete.call_async::<u32>(
        &container, ParameterOverrides::new(),
    ).await.unwrap(), 1);
    assert!(!container.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert_eq!(*container.make::<u32>().unwrap(), 11);

    container.flush();
    assert!(container.get_bindings().is_empty());
    assert!(!container.bound::<u32>());
    assert_eq!(*definition.concrete.call::<u32>(
        &container, ParameterOverrides::new(),
    ).unwrap(), 1);
    container.bind::<u32, _>(|_| Ok(Arc::new(2))).unwrap();
    assert_eq!(*container.make::<u32>().unwrap(), 12);
}
```

`flush()` clears bindings, aliases and reverse aliases, explicit/shared/scoped instances, scoped registration and resolution history. It preserves contextual rules, tags, decorators, resolution/rebinding callbacks, method declarations/bindings and compiled class metadata, as in the base the framework Container. It also detaches caches from every active lifecycle of this container without closing those lifecycles. User values are released after the state change and outside registry/cache locks, allowing destructor reentry. Scoped cache values retire with neutral Facade, parameter and class contexts, both during flush and normal lifecycle closure. A destructor panic leaves the directory cleared and does not poison its lock.

Existing values held by application code and original factory snapshots remain usable. Already admitted constructions may finish with their old values, but cannot repopulate a removed/replaced binding or its history. This base-container operation does not reboot Application providers or clear independent Facade caches. Application teardown has its own lifecycle responsibility.

Rust uses typed definitions and owned ServiceKeys instead of a dynamically typed PHP array. Snapshot lookup is linear in the number of definitions; it is an explicit inspection operation, not part of ordinary request resolution. To support flush across concurrent requests, lifecycles register/unregister a Weak reference under a short mutex operation; flush traverses those active records. Reading a warmed shared binding retains its recipe rather than its cached object.

## Environment Resolver

`resolve_environment_using(Some(Arc::new(callback)))` installs the base container's environment resolver; `None` clears it. Without a resolver, `current_environment_is(query)` returns `Ok(EnvironmentResult::Boolean(false))`. The base container does not parse environment files, match wildcards or reinterpret empty arguments. It forwards `EnvironmentQuery::Name` or `Names` exactly, preserving array order, duplicates and empty strings. Query strings are borrowed for the synchronous callback rather than copied.

The resolver may return `EnvironmentResult::Boolean` or `String`, exactly preserving the supported PHP result shapes. `Result` propagates callback exceptions as container errors. `bool::from(result)` or `bool::from(&result)` applies PHP truthiness: only false, the empty string and the string "0" are false. The example deliberately defines an explicit custom policy that returns a name for an empty array; it does not describe the framework Application's default environment matcher.

```rust
use openrust::{
    container::{Container, EnvironmentQuery, EnvironmentResult},
    contracts::container::Container as _,
};
use std::sync::Arc;

fn main() {
    let container = Container::default();
    assert_eq!(container.current_environment_is("local").unwrap(), false.into());
    container.resolve_environment_using(Some(Arc::new(|query| {
        Ok(match query {
            EnvironmentQuery::Name(name) => (name == "testing").into(),
            EnvironmentQuery::Names([]) => "testing".into(),
            EnvironmentQuery::Names(names) => names.contains(&"testing").into(),
        })
    })));
    assert!(bool::from(container.current_environment_is("testing").unwrap()));
    assert!(bool::from(container.current_environment_is(&["local", "testing"]).unwrap()));
    assert_eq!(container.current_environment_is(&[]).unwrap(),
        EnvironmentResult::from("testing"));
    container.flush();
    assert!(bool::from(container.current_environment_is("testing").unwrap()));
    container.resolve_environment_using(None);
    assert_eq!(container.current_environment_is("testing").unwrap(), false.into());
}
```

Callbacks execute outside the registry lock and may reenter or replace the resolver. An active invocation retains its original callback until it returns. Retired captures are released outside the lock; errors and panics do not poison the registry. Changing or clearing the resolver does not invalidate services or emit resolution events, and `flush` preserves the resolver. Separate containers keep separate policies.

Rust uses borrowed query enums, a typed result enum, Result and an optional Arc callback instead of PHP unions, exceptions and a nullable callable. Callbacks require Send + Sync because one container can serve concurrent requests. Environment checks clone the callback under a short read lock.

For an application, the configuration bootstrap installs the default matcher. See [determining the current environment](../getting-started/configuration.md#determining-the-current-environment).

## Current Container and Offset Access

`Container::get_instance()` returns the current application's container during provider boot, HTTP handling, Artisan execution and application-scoped resolution. Outside an application scope it lazily creates a process default. `Container::set_instance(Some(container))` replaces that default and returns the assigned handle. `Container::set_instance(None)` clears the default. An active application keeps its own container.

`offset_exists` checks whether a key is bound, and `offset_get::<T>` resolves it as `Arc<T>`. Use `offset_set` to bind an existing value or `offset_set_factory` to register a transient factory. The asynchronous equivalents are `offset_get_async` and `offset_set_factory_async`.

```rust
use openrust::container::Container;
use std::sync::Arc;

fn main() {
    let container = Container::get_instance();
    let greeting: Arc<str> = Arc::from("hello");
    container.offset_set("greeting", greeting).unwrap();
    assert!(container.offset_exists("greeting"));
    assert_eq!(&*container.offset_get::<str>("greeting").unwrap(), "hello");

    container.offset_set_factory("answer", |_| Ok(Arc::new(42_u32))).unwrap();
    assert_eq!(*container.offset_get::<u32>("answer").unwrap(), 42);
    container.offset_unset("answer");
    assert!(!container.offset_exists("answer"));
}
```

`offset_unset` removes the exact key's binding, shared instances and resolution record, including cached values in live scopes. Aliases remain registered. Reading an alias whose target was removed returns a resolution error until the target is bound again. Rust uses named methods for fallible, typed lookup and mutation. An in-flight caller can still receive its result after deletion, while subsequent lookups use the current registry.

## Self-building Classes

Implement `SelfBuilding::new_instance` and register a `ClassDescriptor::self_building` when a type needs a static construction method. The descriptor declares independent parameter lists for the ordinary constructor and for `new_instance`. The container calls `new_instance` with the static method's injected arguments.

```rust
use openrust::{
    container::{ClassDescriptor, ConstructorParam, Container, FactoryFuture, ResolvedArgs},
    contracts::container::{Container as _, SelfBuilding},
};
use std::sync::Arc;

struct Report(u32);

impl SelfBuilding for Report {
    #[allow(non_snake_case)]
    fn new_instance(args: ResolvedArgs, container: &Container) -> FactoryFuture<'_, Self> {
        Box::pin(async move {
            let base = container.build::<Self>()?;
            Ok(Arc::new(Self(base.0 + *args.service::<u32>(0)?)))
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    container.instance(Arc::new(7_u32)).unwrap();
    container
        .register_class(
            ClassDescriptor::self_building::<Report>(
                vec![],
                |_| Ok(Arc::new(Report(1))),
                vec![ConstructorParam::class::<u32>("increment")],
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(container.make_async::<Report>().await.unwrap().0, 8);
}
```

Use `make_async` or `build_async` for self-building classes. Constructor overrides supplied to `make_with_async` do not become method arguments. If `new_instance` resolves the same type during construction, the ordinary constructor handles that nested build. An ordinary constructor cycle still produces a resolution error. Use `bind_static_method` with the name `new_instance` to replace the static method for container-driven construction.

## Contextual Attributes

Use `ConstructorParam::with_attribute` to attach a contextual attribute to a constructor, method, or `new_instance` parameter. Implement `ContextualAttribute` to define how it resolves a value. `when_has_attribute` overrides that resolver for a particular container.

```rust
use openrust::{
    container::{
        AttributeParameter, AttributeValue, ClassAttribute, ClassDescriptor,
        ConstructorParam, Container, ParameterValue,
    },
    contracts::container::{BindingResolutionError, Container as _, ContextualAttribute},
};
use std::sync::Arc;

struct Port;
impl ContextualAttribute for Port {
    fn resolve(
        &self,
        _: &Container,
        _: AttributeParameter,
    ) -> Result<AttributeValue, BindingResolutionError> {
        Ok(ParameterValue::new(8080_u16).into())
    }
}

struct Server(u16);
fn main() {
    let container = Container::default();
    let port = ConstructorParam::scalar::<u16>("port")
        .with_attribute(ClassAttribute::contextual(|| Ok(Port)));
    let descriptor = ClassDescriptor::concrete(vec![port], |args| {
        Ok(Arc::new(Server(*args.get::<u16>(0)?)))
    })
    .unwrap();

    container.register_class(descriptor).unwrap();
    assert_eq!(container.make::<Server>().unwrap().0, 8080);
}
```

Constructors check explicit named overrides first, then the first contextual attribute. A null attribute result falls back to ordinary constructor resolution. Method calls check the named override, then the attribute, then the class-name override or normal resolution. Methods preserve an attribute's null result for a nullable slot.

Attach a marker with `ClassAttribute::marker` and use `after_resolving_attribute` to observe attributes, concrete service types or declared trait views. Parameter callbacks run before construction or invocation. Explicit constructor overrides skip those callbacks. Class callbacks run after construction and before extenders and service resolving callbacks. Attribute callbacks are synchronous Rust functions and also run in asynchronous resolution.

### Dispatching Attribute Callbacks

A component that resolves its own arguments can call `fire_after_resolving_attribute_callbacks` with the compiled attributes and the resolved value. Pass `Some(AttributeParameter { .. })` for a parameter, or `None` for a class. Each attribute's `after` runs before registered callbacks. An error stops dispatch and propagates to the caller.

Wrap an object with `ParameterValue::service(arc)` to retain its concrete type. Callbacks can use `event.service::<T>(container)` to obtain the original object or a registered trait view of the same allocation. A value already erased to a trait retains the supplied trait type. Constructors and method calls can apply a declared projection when a contextual attribute supplies the concrete service.

```rust
use openrust::container::{ClassAttribute, Container, ParameterValue};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

struct Audited;
struct Service(AtomicBool);

fn main() {
    let container = Container::default();
    container.after_resolving_attribute::<Audited>(|event, container| {
        let service = event.service::<Service>(container)?.unwrap();
        service.0.store(true, Ordering::Relaxed);
        Ok(())
    });

    let service = Arc::new(Service(AtomicBool::new(false)));
    container.fire_after_resolving_attribute_callbacks(
        &[ClassAttribute::marker(|| Ok(Audited))],
        &ParameterValue::service(service.clone()).into(),
        None,
    ).unwrap();
    assert!(service.0.load(Ordering::Relaxed));
}
```

## Conditional Binding Attributes

Register ordered binding metadata with `register_binding_attributes`. `Bind` matches the application environment, and `BindWhen` evaluates a synchronous predicate with the current container. Use a typed Rust coercion to declare which concrete class implements a service.

```rust
use openrust::{
    container::{Bind, BindWhen, ClassDescriptor, Container},
    contracts::container::Container as _,
};
use std::sync::Arc;

trait Reporter: Send + Sync {
    fn label(&self) -> &'static str;
}

struct LocalReporter;
impl Reporter for LocalReporter {
    fn label(&self) -> &'static str {
        "local"
    }
}

fn main() {
    let container = Container::default();
    container
        .register_class(
            ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(LocalReporter))).unwrap(),
        )
        .unwrap();
    container
        .register_binding_attributes::<dyn Reporter>(vec![
            BindWhen::new(
                Bind::new::<dyn Reporter, LocalReporter>(|reporter| reporter),
                |_| Ok(true),
            )
            .into(),
        ])
        .unwrap();
    assert_eq!(container.make::<dyn Reporter>().unwrap().label(), "local");
}
```

The first matching declaration wins. A wildcard `Bind` is the fallback after all conditions and specific environments have been checked. Environment declarations require an environment resolver, which the application installs during boot. An existing explicit binding takes precedence. A successful selection is registered and reused until the binding is removed. An unmatched `BindWhen` is retried on the next resolution.

## Singleton and Scoped Attributes

Declare a class lifetime with `ClassDescriptor::with_lifetime`. Use `Container::register_lifetime` to declare the lifetime of a service key, including an interface configured with `Bind` or `BindWhen`. If both lifetimes are declared on the same type, the singleton declaration takes precedence.

```rust
use openrust::{
    container::{ClassDescriptor, Container, Lifetime},
    contracts::container::Container as _,
};
use std::sync::Arc;

struct Report;

fn main() {
    let container = Container::default();
    let report = ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Report)))
        .unwrap()
        .with_lifetime(Lifetime::Scoped);
    container.register_class(report).unwrap();

    let first = container.make::<Report>().unwrap();
    assert!(Arc::ptr_eq(&first, &container.make::<Report>().unwrap()));
    container.forget_scoped_instances();
    assert!(!Arc::ptr_eq(&first, &container.make::<Report>().unwrap()));
}
```

`Lifetime::Singleton` reuses the instance in its application container. `Lifetime::Scoped` reuses it within the active request or command lifecycle. Outside an active lifecycle, scoped instances use the container's root cache, which `forget_scoped_instances` can clear.

Sync and async resolution share the same cache. Use async resolution if construction needs asynchronous dependencies. `build` and `build_async` construct directly, and explicit constructor overrides use a fresh instance. An implicit concrete class appears as bound after its instance has been cached. `get_bindings` lists registered bindings, including those selected by `Bind` and `BindWhen`.

## Compiled Interface Views

Declare a trait view on its concrete class with `with_projection(class_projection!(Concrete => dyn Contract))`. Rust checks the coercion at compile time. When a named key or alias selects a binding of that concrete type, `get::<dyn Contract>` and `get_async::<dyn Contract>` can return the same object through its trait view.

```rust
use openrust::{
    class_projection,
    container::{Bind, BindWhen, ClassDescriptor, Container, Lifetime, ServiceKey},
    contracts::container::Container as _,
};
use std::sync::Arc;

trait Report: Send + Sync {}
struct DailyReport;
impl Report for DailyReport {}

fn main() {
    let container = Container::default();
    let descriptor = ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(DailyReport)))
        .unwrap()
        .with_projection(class_projection!(DailyReport => dyn Report))
        .with_lifetime(Lifetime::Singleton);
    container.register_class(descriptor).unwrap();
    container
        .alias(ServiceKey::typed::<DailyReport>(), "daily")
        .unwrap();
    container
        .register_binding_attributes::<dyn Report>(vec![
            BindWhen::new(Bind::implementation::<dyn Report, DailyReport>(), |_| Ok(true))
                .into(),
        ])
        .unwrap();

    let named = container.get::<dyn Report>("daily").unwrap();
    let injected = container.make::<dyn Report>().unwrap();
    assert!(Arc::ptr_eq(&named, &injected));
}
```

A projection preserves the selected service key, lifetime, and object identity. An incompatible view fails before constructing the selected service. To inject the interface by its own key, declare a binding such as `Bind::implementation::<dyn Contract, Concrete>()`. The binding can reuse the projection in a `BindWhen` rule.

## Resolving Events for Interface Views

A callback registered for an interface also matches a concrete class that declares that interface view. Before resolution the container matches the requested class's declaration. During resolving and after resolving it matches the selected object. `ResolutionEvent::value` can return a declared view of that object.

```rust
use openrust::{
    class_projection,
    container::{Bind, BindWhen, ClassDescriptor, Container, ServiceKey},
    contracts::container::Container as _,
};
use std::sync::{Arc, Mutex};

trait Report: Send + Sync {}
struct DailyReport;
impl Report for DailyReport {}

fn main() {
    let container = Container::default();
    container.register_class(
        ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(DailyReport)))
            .unwrap()
            .with_projection(class_projection!(DailyReport => dyn Report)),
    ).unwrap();
    container.register_binding_attributes::<dyn Report>(vec![
        BindWhen::new(Bind::implementation::<dyn Report, DailyReport>(), |_| Ok(true)).into(),
    ]).unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = events.clone();
    container.after_resolving(Some(ServiceKey::typed::<DailyReport>()), move |event, _| {
        let concrete: Arc<dyn Report> = event.value::<DailyReport>().unwrap();
        let view = event.value::<dyn Report>().unwrap();
        assert!(Arc::ptr_eq(&concrete, &view));
        observed.lock().unwrap().push(event.key().clone());
        Ok(())
    }).unwrap();
    container.make::<dyn Report>().unwrap();
    assert_eq!(*events.lock().unwrap(), [ServiceKey::typed::<dyn Report>()]);
}
```

For `Bind` and `BindWhen`, the internal lookup of the concrete implementation skips its own resolving events. Constructor dependencies still raise events normally. The outer event retains the requested service key and can expose the concrete value and its interface view. If an extender replaces the object, the event uses the replacement's available type information.

## Built-in Contextual Attributes

- `Config::<T>::new(key)` reads the current application's configuration. `with_default(value)` applies when the key is missing. An explicit null follows the parameter's normal null handling. Values must deserialize to the declared Rust type.
- `Give::<T>::new(key)` selects a concrete service or a declared trait view. `with_parameters` passes a separate set of constructor arguments to that service. Use asynchronous resolution for implementations that need it.
- `Tag::<T>::new(tag)` injects `TaggedServices<T>`. It resolves entries lazily and preserves registration order and duplicates. Supply the original container to `iter` or `collect_async` to resolve in the active lifecycle.
- OpenRust's `CurrentResolving` attribute exposes `currently_resolving()` as a typed `ServiceKey`. It supplies null outside a constructor or method context.

```rust
use openrust::{
    config::Repository,
    container::{
        ClassAttribute, ClassDescriptor, ConstructorParam, Container, TaggedServices,
        attributes::{Config, Tag},
    },
    contracts::{config::Repository as ConfigRepository, container::Container as _},
};
use std::{collections::BTreeMap, sync::Arc};

struct Reports(u16, Arc<TaggedServices<u32>>);

fn main() {
    let container = Container::default();
    container
        .instance_named::<dyn ConfigRepository>("config", Arc::new(Repository::new(BTreeMap::new())))
        .unwrap();
    container.instance_named("daily", Arc::new(7_u32)).unwrap();
    container.tag(["daily"], ["reports"]);

    let descriptor = ClassDescriptor::concrete(
        vec![
            ConstructorParam::scalar::<u16>("port").with_attribute(
                ClassAttribute::contextual(|| Ok(Config::new("reports.port").with_default(8080_u16))),
            ),
            ConstructorParam::scalar::<TaggedServices<u32>>("services").with_attribute(
                ClassAttribute::contextual(|| Ok(Tag::<u32>::new("reports"))),
            ),
        ],
        |args| Ok(Arc::new(Reports(*args.get(0)?, args.get(1)?))),
    ).unwrap();
    container.register_class(descriptor).unwrap();

    let reports = container.make::<Reports>().unwrap();
    assert_eq!(reports.0, 8080);
    assert_eq!(*reports.1.iter(&container).unwrap().next().unwrap().unwrap(), 7);
}
```

Custom attributes can implement `resolve_async` when they need to await a dependency. An existing `when_has_attribute` override retains precedence in both resolution modes.

### Service Manager Attributes {#service-manager-attributes}

Use `Cache`, `Database` or `DB`, `Log`, and `Storage` on constructor or method parameters. Each attribute resolves the manager in the current container and selects its default service or the name passed to `new`.

The injected types are `Arc<cache::Repository>`, `Arc<database::Connection>`, `Arc<log::Logger>`, and `Arc<filesystem::FilesystemAdapter>`. `Cache::memoized()` shares memoized reads within the current scope. `Log::named()` changes the injected logger name while reusing the channel handlers.

```rust
use openrust::{
    container::{ClassAttribute, ConstructorParam, attributes::Storage},
    filesystem::FilesystemAdapter,
};

pub fn private_disk_parameter() -> ConstructorParam {
    ConstructorParam::class::<FilesystemAdapter>("disk")
        .with_attribute(ClassAttribute::contextual(|| Ok(Storage::new("private"))))
}
```

### Authentication Attributes {#authentication-attributes}

Use `Auth` to inject a `GuardHandle` and `Authenticated` or `CurrentUser` to inject an `Arc<dyn Authenticatable>`. All three accept a guard name through `new(name)`. Use `default()` for the guard selected for the request, or the application default if none was selected. Declare user parameters nullable with a null default when guests may reach a controller.

```rust
use openrust::{
    container::{ClassAttribute, ConstructorParam, ParameterValue, attributes::CurrentUser},
    contracts::auth::Authenticatable,
};

pub fn current_user_parameter() -> ConstructorParam {
    ConstructorParam::class::<dyn Authenticatable>("user")
        .nullable()
        .with_default(|| Ok(ParameterValue::null()))
        .with_attribute(ClassAttribute::contextual(|| Ok(CurrentUser::new("api"))))
}
```

Controller construction can resolve these attributes directly. For asynchronous container calls in a closure handler, use `request.with_container(container.make_async::<T>()).await` to make the same request available to the resolver. Bind the injected guard with `guard.bind(request)?` to run its operations against the live session and cached identity. The handle is valid during the originating HTTP dispatch.

### Request and Route Parameters

Use `RequestAttribute::new("principal")` to inject a value that server-side code stored with `request.attributes().insert("principal", ParameterValue::new(value))`. Names match exactly. Headers, query parameters, and form fields are separate sources.

`RouteParameter::new("project")` uses the given route parameter name. `RouteParameter::default()` uses the name of the parameter being injected. The attribute returns the matched string until `SubstituteBindings` resolves a model, then supplies that same model. Declare the corresponding Rust type in the constructor or method metadata.

A missing value or the absence of an active HTTP request supplies null to the container's normal parameter resolution. The attributes are available during HTTP dispatch. Response body streams are polled after dispatch, so pass any values they need to the stream explicitly.

```rust
use openrust::{
    container::{
        ClassAttribute, ClassDescriptor, ConstructorParam, Container,
        attributes::{RequestAttribute, RouteParameter},
    },
    contracts::container::Container as _,
};
use std::sync::Arc;

struct ProjectContext {
    has_principal: bool,
    has_project: bool,
}

fn main() {
    let container = Container::default();
    container.register_class(ClassDescriptor::concrete(
        vec![
            ConstructorParam::scalar::<String>("principal").nullable().with_attribute(
                ClassAttribute::contextual(|| Ok(RequestAttribute::new("principal"))),
            ),
            ConstructorParam::scalar::<String>("project").nullable().with_attribute(
                ClassAttribute::contextual(|| Ok(RouteParameter::default())),
            ),
        ],
        |args| Ok(Arc::new(ProjectContext {
            has_principal: !args.value(0)?.is_null(),
            has_project: !args.value(1)?.is_null(),
        })),
    ).unwrap()).unwrap();

    let context = container.make::<ProjectContext>().unwrap();
    assert!(!context.has_principal && !context.has_project);
}
```

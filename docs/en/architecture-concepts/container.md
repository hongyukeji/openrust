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

Scoped bindings, deferred services, contextual binding, extenders and resolution callbacks remain in progress in container unit 01-02. The binding and resolving behavior above does not imply those features are implemented.

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

These operations do not clear a facade's own cached root; use its cache-clearing API when required. Scoped service eviction is delivered with the scoped lifecycle portion of the container unit.

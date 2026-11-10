# Service Providers

## Registering Providers

`bootstrap/providers.rs` lists the application service providers shared by HTTP and Artisan. It registers the default `AppServiceProvider` from `app/providers/app_service_provider.rs`. This provider supplies the application’s `register` and `boot` hooks.

The selected kernel loads configuration, runs all eager providers’ `register` methods, and then runs their `boot` methods. See the [request lifecycle](lifecycle.md).

## Writing Service Providers

Implement `openrust::support::ServiceProvider` and list the provider in `bootstrap/providers.rs`. Override `register` to bind services and `boot` to initialize them after every provider has registered. Both hooks have successful no-op defaults; implement only the hooks your provider needs. `register` and `boot` return asynchronous futures, allowing registration dependencies and initialization to await I/O.

Providers are deduplicated by their concrete Rust type, retaining the first registration. Successful bootstrap is idempotent. If registration or boot fails, later stages do not run, and the same application cannot retry a partially completed bootstrap. The error identifies the provider and phase while preserving the original source for reporting. Construct a new application to retry initialization.

Provider hooks run inside their application's facade scope. HTTP and Artisan share the same provider lifecycle, while their execution scopes and containers remain separate between applications. See [container bindings](container.md#binding) and [facades](facades.md#how-facades-work).

## Deferred Providers

Implement `openrust::contracts::support::DeferrableProvider` and declare exact service keys in `provides()`. Register a `DeferredProvider::new(factory)` in the existing provider list. The compiled factory replaces PHP class-name construction; static metadata does not instantiate the provider. Type deduplication uses the underlying provider type. HTTP leaves unused providers unloaded; the Console Kernel calls `loadDeferredProviders()` before registering commands.

`Application::make::<T>().await` and `get::<T>(key).await` wait for a deferred provider. Container callers use `makeAsync` / `getAsync`; facades offer `getFacadeRootAsync`. Synchronous lookups of an uninitialized deferred service return `DeferredServiceRequiresAsync` rather than blocking Tokio. Synchronous binding factories must receive already initialized dependencies; they cannot await another provider. Both `register` and `boot` return futures, so a registration hook can await `app.make()` for another provider.

Use an [asynchronous factory](container.md#asynchronous-factories) to resolve deferred dependencies automatically inside construction.

```rust
use std::{error::Error, sync::Arc};
use openrust::{
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _, support::DeferrableProvider},
    foundation::Application,
    http::HttpFuture,
    support::{DeferredProvider, ProviderResult, ServiceProvider},
};

struct Client;
struct ClientProvider;
impl DeferrableProvider for ClientProvider {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<Client>()]
    }
}
impl ServiceProvider for ClientProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().singletonAsync::<Client, _>(|_| Box::pin(async {
                tokio::task::yield_now().await;
                Ok(Arc::new(Client))
            })).await?;
            Ok(())
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut app = Application::configure(std::env::current_dir()?)
        .withProviders(vec![Box::new(DeferredProvider::new(|| ClientProvider))])
        .create()?;
    app.boot().await?;
    assert!(app.isDeferredService(ServiceKey::typed::<Client>()));
    let client = app.make::<Client>().await?;
    assert!(Arc::ptr_eq(&client, &app.make::<Client>().await?));
    Ok(())
}
```

An existing explicit instance bypasses an unused loader. Aliases are normalized before loading. Concurrent callers share registration and wait for boot; declared services remain inaccessible to other callers while boot is pending. The owner may resolve its own registered service during boot. A failed or cancelled boot leaves an explicit failure gate; create a new application to retry it. This concurrent readiness protection is a Rust adaptation beyond PHP’s sequential execution. Already contributed container values are retained.

Loading consumes the trigger key only. Sibling keys remain in the deferred directory until `loadDeferredProviders()` completes. Registration errors retain contributions and consume that key; a later unconsumed key retries with a fresh compiled factory instance. Registering an already constructed deferred provider with `isDeferred()` and `provides()` reuses that object on retries; use `DeferredProvider` when constructor behavior matters.

A provider first loaded before the boot phase joins both the booting callbacks and the registered provider list, so its boot hook runs twice, as in the fixed Laravel baseline. First use during an eager provider’s boot joins the remaining provider boot list once. First use after application boot waits for registration and boot before returning the service. Event-based loading and additional provider lifecycle callbacks belong to later delivery units.

Register compiled constructor metadata in `register` when a concrete type should support automatic injection without a service factory. This does not mark the type bound or instantiate it. See [automatic constructor injection](container.md#automatic-constructor-injection).

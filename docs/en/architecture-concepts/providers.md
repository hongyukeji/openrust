# Service Providers

## Registering Providers

`bootstrap/providers.rs` lists the application service providers shared by HTTP and Artisan. It registers the default `AppServiceProvider` from `app/providers/app_service_provider.rs`. This provider supplies the application’s `register` and `boot` hooks.

The selected kernel loads configuration, runs all providers’ `register` methods, and then runs their `boot` methods. See the [request lifecycle](lifecycle.md).

## Writing Service Providers

Implement `openrust::support::ServiceProvider` and list the provider in `bootstrap/providers.rs`. Override `register` to bind services and `boot` to initialize them after every provider has registered. Both hooks have successful no-op defaults; implement only the hooks your provider needs. `boot` returns an asynchronous future so initialization can await I/O.

Providers are deduplicated by their concrete Rust type, retaining the first registration. Successful bootstrap is idempotent. If registration or boot fails, later stages do not run, and the same application cannot retry a partially completed bootstrap. The error identifies the provider and phase while preserving the original source for reporting. Construct a new application to retry initialization.

Provider hooks run inside their application's facade scope. HTTP and Artisan share the same provider lifecycle, while their execution scopes and containers remain separate between applications. See [container bindings](container.md#binding) and [facades](facades.md#how-facades-work).

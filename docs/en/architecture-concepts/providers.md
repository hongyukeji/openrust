# Service Providers

## Registering Providers

`bootstrap/providers.rs` lists the application service providers shared by HTTP and Artisan. It registers the default `AppServiceProvider` from `app/providers/app_service_provider.rs`. This provider supplies the application’s `register` and `boot` hooks.

The selected kernel loads configuration, runs all providers’ `register` methods, and then runs their `boot` methods. See the [request lifecycle](lifecycle.md).

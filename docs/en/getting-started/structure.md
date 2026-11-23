# Directory Structure

## Application directories

The root `main.rs` connects Tokio and `bootstrap::create_application`. `Cargo.toml` and `Cargo.lock` define the application, workspace and locked dependencies. Application code is in `app/`, compiled configuration loaders in `config/`, and Web and console declarations in `routes/`.

The framework lives in `vendor/openrust/framework/`. Views are in `resources/views/`, website files in `public/`, and the public storage disk in `storage/app/public/`. Tests belong in the root application's or the owning crate's `tests/` directory.

## The Bootstrap Directory

The application’s `bootstrap/` directory contains the declarations used to construct the application.

`bootstrap/app.rs` declares configuration, routes, middleware and console commands. `bootstrap/mod.rs` exposes `bootstrap::create_application` to the binary entry point. `bootstrap/providers.rs` lists the application’s [service providers](../architecture-concepts/providers.md#registering-providers).

The bootstrap declares Web and console routes and the health path with `with_routing`. Rust uses the `Routing` builder for these options and registers compiled configuration, providers and commands through `with_config`, `with_providers` and `with_commands`.

For the order in which the framework uses these declarations, see the [request lifecycle](../architecture-concepts/lifecycle.md).
## Application Builder Hooks

`Application::configure` accepts `with_bindings::<T, _>`, `with_singletons::<T, _>`, and `with_scoped_singletons::<T, _>` factories. Chain one registration per Rust type; factories return `Result<Arc<T>, BindingResolutionError>`. Transient bindings resolve every time, singletons belong to the application, and scoped singletons belong to the current request or queue task.

`registered`, `booting`, and `booted` run in that order around provider boot, and successful application boot is idempotent. A failed hook stops subsequent phases and makes boot fail. `with_schedule` registers a lazy schedule configurator; it does not execute tasks during application boot. `prefers_json_responses(true)` enables JSON preference at the booted phase for broad Accept headers; an explicit exception JSON policy keeps priority.

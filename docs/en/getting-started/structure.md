# Directory Structure

## Application directories

The root `main.rs` connects Tokio and `bootstrap::createApplication`. `Cargo.toml` and `Cargo.lock` define the application, workspace and locked dependencies. Application code is in `app/`, compiled configuration loaders in `config/`, and Web and console declarations in `routes/`.

The framework lives in `vendor/openrust/framework/`. Views are in `resources/views/`, website files in `public/`, and the public storage disk in `storage/app/public/`. Tests belong in the root application's or the owning crate's `tests/` directory.

## The Bootstrap Directory

The application’s `bootstrap/` directory contains the declarations used to construct the application.

`bootstrap/app.rs` declares configuration, routes, middleware and console commands. `bootstrap/mod.rs` exposes `bootstrap::createApplication` to the binary entry point. `bootstrap/providers.rs` lists the application’s [service providers](../architecture-concepts/providers.md#registering-providers).

The bootstrap declares Web and console routes and the health path with `withRouting`. Rust uses the `Routing` builder for these options and registers compiled configuration, providers and commands through `withConfig`, `withProviders` and `withCommands`.

For the order in which the framework uses these declarations, see the [request lifecycle](../architecture-concepts/lifecycle.md).

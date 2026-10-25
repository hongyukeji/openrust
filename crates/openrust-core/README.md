# openrust-core

Application kernel of the [OpenRust](https://github.com/hongyukeji/openrust) framework.

This crate owns the contracts every other OpenRust crate builds on:

- [`Application`] — the trait a project implements once to describe how its app is assembled.
- [`AppBuilder`] / [`App`] / [`AppState`] — compile-time assembled application with a typed service registry. Handlers and commands obtain services explicitly; there is no runtime container or reflection.
- [`ServiceProvider`] — the single extension point for framework capabilities, application modules and third-party packages: register services, commands and hooks, then boot once everything is wired.
- [`Command`] — runtime commands exposed by the application binary (`serve`, `migrate`, `queue:work`, …).
- [`Error`] / [`Result`] / [`ProblemDetails`] — one error type for the whole framework, rendered as RFC 9457 `application/problem+json` when it reaches HTTP.
- [`config`] — layered configuration: crate defaults → `config/app.toml` → `config/<env>.toml` → environment variables prefixed `APP__`.
- [`lifecycle`] — bootstrap sequence and graceful-shutdown signal.

`openrust-core` deliberately depends only on `axum-core` (for `IntoResponse`), `http`, `serde`, `figment`, `tokio` and `tracing`. HTTP routing, databases and other capabilities live in their own crates. Applications depend on the facade crate `openrust`, not on this crate directly.

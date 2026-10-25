# openrust-core

Application kernel of the [OpenRust](https://github.com/hongyukeji/openrust) framework.

This crate owns the contracts every other OpenRust crate builds on:

- [`Application`] — the trait a project implements once to describe how its app is assembled.
- [`AppBuilder`] / [`App`] / [`AppState`] — compile-time assembled application with a typed service registry. Handlers and commands obtain services explicitly; there is no runtime container or reflection.
- [`Plugin`] — the single extension point for framework capabilities, application modules and third-party packages: build services, commands and hooks, then finish once everything is wired.
- [`Command`] — runtime commands exposed by the application binary (`serve`, `migrate`, `queue work`, …). Commands reject extra arguments by default; commands that take them implement `accepts_args`.
- [`Error`] / [`Result`] / [`ProblemDetails`] — one error type for the whole framework, rendered as RFC 9457 `application/problem+json` when it reaches HTTP.
- [`config`] — layered configuration: crate defaults → `config/default.toml` → `config/<env>.toml` → environment variables prefixed `OPENRUST_`. Bootstrap loads `.env` outside production. A process-level `OPENRUST_ENV=production` skips `.env`; when unset, `.env` may select the environment.
- [`lifecycle`] — bootstrap sequence and graceful-shutdown signal.

`openrust-core` deliberately depends only on `axum-core` (for `IntoResponse`), `http`, `serde`, `figment`, `tokio` and `tracing`. HTTP routing, databases and other capabilities live in their own crates. Applications depend on the facade crate `openrust`, not on this crate directly.

`ServerConfig` includes `[server.compression]` (enabled by default) and `[server.cors]` (disabled by default). The latter defaults to an empty origin list, GET/POST/PUT/PATCH/DELETE/OPTIONS methods, `content-type`/`authorization`/`x-request-id` request headers, `x-request-id` exposed header, credentials disabled and a 600-second preflight cache. Environment variables use `OPENRUST_SERVER__CORS__ENABLED=true`; list values use Figment array syntax, for example `OPENRUST_SERVER__CORS__ALLOWED_ORIGINS='["https://example.com", "https://other.example"]'`.

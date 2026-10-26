# Application core

Application kernel of the [OpenRust](https://github.com/hongyukeji/openrust) framework.

This crate owns the contracts every other OpenRust crate builds on:

- [`Application`] — the trait a project implements once to describe how its app is assembled.
- [`AppBuilder`] / [`App`] / [`AppState`] — compile-time assembled application with a typed service registry. Handlers and commands obtain services explicitly; there is no runtime container or reflection.
- [`Plugin`] — the single extension point for framework capabilities, application modules and third-party packages: build services, commands and hooks, then finish once everything is wired.
- [`Command`] — runtime commands exposed by the application binary (`serve`, `migrate`, `queue work`, …). Commands reject extra arguments by default; commands that take them implement `accepts_args`.
- [`Error`] / [`Result`] / [`ApiResponse<T>`] / [`PageMeta`] — one error type, shared JSON envelope, and typed pagination metadata.
- [`RateLimiter`] / [`RateLimitStore`] — one fixed-window quota service contract. Store failures propagate, over-limit requests return 429 with `Retry-After`, and each implementation documents its sharing and atomicity boundary.
- `config` — typed configuration definitions are organized by domain in `openrust::config::{app,api,auth,database,logger,mailer,server}` and merged by one layered loader: crate defaults → `config/default.toml` → `config/<env>.toml` → non-production `.env` values → process environment. `APP_CONFIG_DIR` selects the runtime TOML directory (default: `config` relative to the working directory); `src/config/` contains Rust code only. Bootstrap reads `APP_ENV` from `.env` only when the process has not set it; production skips the entire `.env` file. `Application::config_namespace()` can select another valid prefix, such as `SHOP_`; only the selected namespace is read. Process environment variables take precedence, and the current capability-module paths remain re-exports for compatibility.
- [`lifecycle`] — bootstrap sequence and graceful-shutdown signal.

The application kernel lives in the root `openrust` package at `src/core/`; `config`, `http`, `db`, `auth` and `mail` are sibling Rust modules in that same package. Applications depend on the public `openrust` package. CLI, proc-macro and reusable test tools remain separate workspace packages for distribution and tooling boundaries.

`ServerConfig` includes `[server].trusted_proxy_ranges` (empty by default), `[server.compression]` (enabled by default) and `[server.cors]` (disabled by default). Trusted proxy entries accept IPs or CIDRs and are used by the HTTP `ClientIp` extractor; arbitrary forwarding headers are ignored unless the direct socket peer is trusted. CORS defaults to an empty origin list, GET/POST/PUT/PATCH/DELETE/OPTIONS methods, `content-type`/`authorization`/`x-request-id` request headers, `x-request-id` exposed header, credentials disabled and a 600-second preflight cache. Environment variables use `APP_SERVER__CORS__ENABLED=true`; list values use Figment array syntax, for example `APP_SERVER__CORS__ALLOWED_ORIGINS='["https://example.com", "https://other.example"]'`.

HTTP responses use `ApiResponse<T>` with `success`, stable `code`, safe `message`, `data`, optional typed `PageMeta`/`errors`, and `request_id`. Errors use `Error::custom(code, message)` (semantic status defaults to 400) or `Error::custom_with_status(status, code, message)`. `Error::rate_limited(seconds)` emits `Retry-After`; `Error::validation_with_detail` supplies 422 field errors. 5xx details are hidden. The HTTP middleware scopes immutable `request_id` and `[api] http_status` values to each request. `always_ok` is the default and returns HTTP 200; `semantic` keeps the status table in [ADR-008](../../.agents/project/decisions/adr-008-api-conventions.md). Set `APP_API__HTTP_STATUS=semantic` to override configuration.

`RateLimiter::check(key, limit, window)` consumes one hit from the registered `RateLimitStore`, returning remaining quota or a 429 error with `Retry-After`. The core contract does not choose HTTP identity extraction or middleware policy. Callers must namespace keys by policy and use the same window for each key; store errors are not treated as quota misses. SHA-256 storage by the database store avoids directly storing keys but does not prevent guessing low-entropy values.

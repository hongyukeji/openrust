# Changelog

All notable changes to OpenRust are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). OpenRust has not published a release yet.

## [Unreleased]

### Fixed

- Preserved typed `PageMeta` in API responses and documented empty CORS preflight bodies while keeping ordinary cross-origin errors wrapped.

### Added

- Added OpenRust `Json` / `Path` / `Query` extractors, `Created::with_location`, and JSON envelope fallbacks for common HTTP rejections.

- Added the non-published `apps/server` integration host as the workspace default for `cargo run`.
- Established the Cargo workspace, shared lint policy, dual license, CI baseline and framework architecture decisions.
- Added the application core with layered configuration, typed service registry, plugin lifecycle, runtime commands and structured HTTP errors and configurable JSON envelopes.
- Added HTTP routing, resource actions, typed responses, validation and injection extractors, health route, middleware, CORS and compression.
- Added the facade application runner, a minimal example and a CLI that forwards runtime commands to the project binary.
- Added contributor, security and community guidance.

### Changed

- Switched all OpenRust JSON success and error responses to `ApiResponse<T>` and added `[api] http_status` (`always_ok` or `semantic`), request ID body/header parity, and rate-limit retry headers.

- Switched the facade prelude to OpenRust extractors and aligned validation, timeout and unmatched-route responses with ADR-008.

- Adopted ADR-006 names across the implemented API: `Plugin::build/finish`, `AppBuilder::plugin/insert/get`, `AppState::get/try_get`, `HttpPlugin`, `Routes::nest` and resource actions `list/get/create/update/delete`.
- Switched runtime configuration to `config/default.toml`, `OPENRUST_ENV` and `OPENRUST_<SECTION>__<KEY>`; `DATABASE_URL` and `PORT` map to the corresponding sections, with prefixed values taking priority.
- Changed the configuration command to `config show` and documented the future `openrust generate` CLI.

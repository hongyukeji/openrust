# openrust-server

Framework integration host for the currently implemented OpenRust capabilities. `Server::build` installs `HttpPlugin`, including `/health`, request IDs, configurable compression and CORS. `/` reports the server name and version. No external service is required at M1.

Run from the repository root:

```bash
cargo run
cargo run -p openrust-server -- config show
cargo run -p openrust-server -- help
cargo test -p openrust-server
```

The server reads `apps/server/config/default.toml` and `apps/server/config/development.toml`. For repository development, `.cargo/config.toml` sets `OPENRUST_CONFIG_DIR` to that directory when running Cargo. In an application, the default is `config/` relative to the working directory; `OPENRUST_CONFIG_DIR` can select another directory. `OPENRUST_ENV` selects the environment. Environment variables override file values; `OPENRUST_SERVER__PORT=4000 cargo run` changes the port, and `PORT=4000 cargo run` is the common shorthand. If both are set, `OPENRUST_SERVER__PORT` wins. `config show` masks keys that look like secrets.

New framework capabilities are connected here in the same feature change, with an integration test. Capabilities without external services are always enabled; external services use a forwarded feature when implemented. See [ADR-007](../../.agents/project/decisions/adr-007-aggregate-server-entry.md).

`/` returns the `ApiResponse<T>` JSON envelope; `/health` remains raw `{"status":"ok"}`. `[api] http_status` defaults to `always_ok`; set `OPENRUST_API__HTTP_STATUS=semantic` for semantic HTTP status codes. Every API envelope has a `request_id` matching `x-request-id`.

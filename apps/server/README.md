# openrust-server

Framework integration host for the currently implemented OpenRust capabilities. It installs the HTTP plugin through `openrust::run`, including `/health`, request IDs, configurable compression and CORS. `/` reports the server name and version. No external service is required at M1.

Run from the repository root:

```bash
cargo run
cargo run -p openrust-server -- config show
cargo run -p openrust-server -- help
cargo test -p openrust-server
```

The server reads `apps/server/config/default.toml` and `apps/server/config/development.toml`; the repository root `config` symlink makes them available to `cargo run` from the root. `OPENRUST_ENV` selects the environment. Environment variables override file values; `OPENRUST_SERVER__PORT=4000 cargo run` changes the port, and `PORT=4000 cargo run` is the common shorthand. If both are set, `OPENRUST_SERVER__PORT` wins. `config show` masks keys that look like secrets.

New framework capabilities are connected here in the same feature change, with an integration test. Capabilities without external services are always enabled; external services use a forwarded feature when implemented. See [ADR-007](../../.agents/project/decisions/adr-007-aggregate-server-entry.md).

# openrust-server

Framework integration host for the currently implemented OpenRust capabilities. `Server::build` installs `HttpPlugin`, `I18nPlugin`, and the mailer. HTTP error messages follow the supported `Accept-Language` request preference, with English as the configured default. `/` reports the server name and version. No external service is required for these paths.

Run from the repository root:

```bash
cargo run -p openrust-server
cargo run -p openrust-server -- config show
cargo run -p openrust-server -- help
cargo test -p openrust-server
```

The server reads `apps/server/config/default.toml` and `apps/server/config/development.toml`. For repository development, `.cargo/config.toml` sets `APP_CONFIG_DIR` to that directory when running Cargo. In an application, the default is `config/` relative to the working directory; `APP_CONFIG_DIR` can select another directory. `APP_ENV` selects the environment. Environment variables override file values; `APP_SERVER__PORT=4000 cargo run` changes the port, and `PORT=4000 cargo run` is the common shorthand. If both are set, `APP_SERVER__PORT` wins. `config show` masks keys that look like secrets.

New framework capabilities are connected here in the same feature change, with an integration test. Capabilities without external services are always enabled; external services use a forwarded feature when implemented. See [ADR-007](../../.agents/project/decisions/adr-007-aggregate-server-entry.md).

Enable the `auth` feature to compile the authentication facade path and run its application-level integration test with a custom authenticator:

```bash
cargo test -p openrust-server --features auth --test auth
```

Applications using `AuthPlugin::new()` must register `DatabasePlugin` first and include `AuthMigration` in their existing SeaORM migrator. Custom `Authenticator` implementations do not require a database.

The facade auth test also verifies that `AuthMode::Jwt` authenticates without registering `SessionPlugin`; the test uses explicit in-memory key configuration and never writes a secret to repository configuration.

`/` and `/health` return the `ApiResponse<T>` JSON envelope; health status is in `data.status`. `[api] http_status` defaults to `always_ok`; set `APP_API__HTTP_STATUS=semantic` for semantic HTTP status codes. Every API envelope has a `request_id` matching `x-request-id`.

The `[i18n]` section in `config/default.toml` configures `default_locale` and `fallback_locale`. Requests can send `Accept-Language: zh-CN`; supported framework messages then render in Simplified Chinese while response codes and validation field paths remain stable. Applications can register their own JSON catalog with `I18nPlugin` and inject `RequestLocale` in handlers.

`Server` registers `SmsPlugin` alongside `MailerPlugin`. Local delivery defaults to content-redacted log drivers; production rejects them. Enable `sms-twilio` for the Twilio adapter and configure API credentials through process environment or local `.env` (never commit populated `.env`). Mailpit local SMTP steps and the loopback-only STARTTLS exception are documented in the Mail README.

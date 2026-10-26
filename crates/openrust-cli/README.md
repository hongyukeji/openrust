# openrust-cli

The global `openrust` command of the [OpenRust](https://github.com/hongyukeji/openrust) framework.

```bash
cargo install openrust-cli
```

Create and run an application:

```bash
openrust new blog --db postgres --auth
cd blog
openrust generate model post title:string published_at:date_time?
openrust generate migration comments body:text post:references?
openrust g migration labels slug:string
```

`new` accepts a directory name or destination path and creates the OpenRust starter layout and HTTP application. The generated app registers the HTTP and mail plugins; mail delivery defaults to a log driver that does not log message contents. Without `--db`, it creates a project without database support. `--db postgres` or `--db mysql` also creates a migration crate, registers database, migration and seed plugins, and selects the matching backend feature. Copy `.env.example` to `.env` for local overrides; the example documents common server/API overrides, optional SMTP settings, and, when enabled, database/JWT values. External process variables take precedence, and production never loads `.env`. Set a valid `DATABASE_URL`, then run `openrust migrate` before starting a database-backed service. The generated `.gitignore` excludes `.env` and environment-specific variants while keeping example files trackable. `dev` runs `cargo run --quiet -- serve` with `APP_ENV=development`; `serve` runs the same project command without changing the environment.

`generate model` creates a SeaORM entity in `src/models/_entities/` and registers its module. `generate migration` creates a table migration and adds it to the app migration registry. Both accept `string`, `text`, `integer`, `big_integer`, `boolean`, `date_time`, `json`, and `references` fields; append `?` for nullable columns. Reference fields use `<name>_id` and target the pluralized referenced table's `id` column. Model generation uses a singular model name and a simple English plural form for the table; migration generation accepts the table name directly. The referenced table must exist before its foreign key migration runs.

Any other command is forwarded to the application binary in the current project (`openrust migrate` runs `cargo run --quiet -- migrate`), so a project has one entry point for framework commands and application commands.

The default template has no database dependency. Database generators require a project created with `openrust new --db`. The optional `--auth` flag requires `--db postgres` or `--db mysql`; it enables the facade auth feature, registers `JwtPlugin`, `AuthPlugin`, and `AuthFlowsPlugin`, adds account, refresh, verification-code, Policy and shared rate-limit migrations, and protects `/api/me` with JWT Bearer authentication and `/api/me/pat` with database-backed PAT authentication. It also generates JSON endpoints for registration, email verification, login, refresh, logout and password reset. Set `APP_AUTH__JWT__KEYS__LOCAL` in `.env` to the output of `openssl rand -base64 32` before starting the app. The template never embeds a signing secret. Generated mail uses the safe `log` driver by default, which does not expose verification codes; configure SMTP to deliver them. `[server].trusted_proxy_ranges` defaults to an empty list; configure the reverse proxy IPs/CIDRs before relying on forwarded client addresses for per-client rate isolation.

Auth-enabled projects include a real HTTP integration test at `tests/auth_flows.rs`. It starts a loopback listener, uses an isolated database and fake mail driver, and exercises the generated auth endpoints. Set `OPENRUST_TEST_DATABASE_URL` to an existing PostgreSQL or MySQL database whose test account can create and drop databases before running `cargo test`; when it is unset, the test reports that the database integration test was skipped.

Before opening a generated project, verify its source and build with `cargo fmt --all --check`, `cargo check`, and `cargo test`. The auth integration test performs real database operations while sending verification messages through its test mail driver.

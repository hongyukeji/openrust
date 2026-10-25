# openrust-cli

The global `openrust` command of the [OpenRust](https://github.com/hongyukeji/openrust) framework.

```bash
cargo install openrust-cli
```

Create and run an application:

```bash
openrust new blog --db postgres
cd blog
openrust generate model post title:string published_at:date_time?
openrust generate migration comments body:text post:references?
openrust g migration labels slug:string
```

`new` accepts a directory name or destination path and creates the OpenRust starter layout and HTTP application. The generated app registers the HTTP and mail plugins; mail delivery defaults to a log driver that does not log message contents. Without `--db`, it creates a project without database support. `--db postgres` or `--db mysql` also creates a migration crate, registers database, migration and seed plugins, and selects the matching backend feature. Copy `.env.example` to `.env`, set a valid `DATABASE_URL`, then run `openrust migrate` before starting the service. The generated `.gitignore` excludes `.env` and environment-specific variants while keeping example files trackable. `dev` runs `cargo run --quiet -- serve` with `OPENRUST_ENV=development`; `serve` runs the same project command without changing the environment.

`generate model` creates a SeaORM entity in `src/models/_entities/` and registers its module. `generate migration` creates a table migration and adds it to the app migration registry. Both accept `string`, `text`, `integer`, `big_integer`, `boolean`, `date_time`, `json`, and `references` fields; append `?` for nullable columns. Reference fields use `<name>_id` and target the pluralized referenced table's `id` column. Model generation uses a singular model name and a simple English plural form for the table; migration generation accepts the table name directly. The referenced table must exist before its foreign key migration runs.

Any other command is forwarded to the application binary in the current project (`openrust migrate` runs `cargo run --quiet -- migrate`), so a project has one entry point for framework commands and application commands.

The default template has no database dependency. Database generators require a project created with `openrust new --db`.

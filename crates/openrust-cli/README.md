# openrust-cli

The global `openrust` command of the [OpenRust](https://github.com/hongyukeji/openrust) framework.

```bash
cargo install openrust-cli
```

Create and run an application:

```bash
openrust new blog
openrust new blog --db postgres
cd blog
openrust dev
```

`new` accepts a directory name or destination path and creates the OpenRust starter layout and HTTP application. `--db postgres` or `--db mysql` also creates a migration crate, registers database, migration and seed plugins, and selects the matching backend feature. Copy `.env.example` to `.env`, set a valid `DATABASE_URL`, then run `openrust migrate` before starting the service. `dev` runs `cargo run --quiet -- serve` with `OPENRUST_ENV=development`; `serve` runs the same project command without changing the environment.

Any other command is forwarded to the application binary in the current project (`openrust migrate` runs `cargo run --quiet -- migrate`), so a project has one entry point for framework commands and application commands.

The default template has no database dependency. `generate` / `g` is added in a later milestone.

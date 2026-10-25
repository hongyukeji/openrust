# openrust-cli

The global `openrust` command of the [OpenRust](https://github.com/hongyukeji/openrust) framework.

```bash
cargo install openrust-cli
```

Create and run an application:

```bash
openrust new blog
cd blog
openrust dev
```

`new` accepts a directory name or destination path and creates the OpenRust starter layout and HTTP application. `dev` runs `cargo run --quiet -- serve` with `OPENRUST_ENV=development`; `serve` runs the same project command without changing the environment.

Any other command is forwarded to the application binary in the current project (`openrust migrate` runs `cargo run --quiet -- migrate`), so a project has one entry point for framework commands and application commands.

The M1 template has no database dependency. Database selection (`new --db`) and `generate` / `g` are added in later milestones.

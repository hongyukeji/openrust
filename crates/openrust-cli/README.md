# openrust-cli

The global `openrust` command of the [OpenRust](https://github.com/hongyukeji/openrust) framework.

```bash
cargo install openrust-cli
```

Any command that is not a built-in is forwarded to the application binary in the current project (`openrust migrate` runs `cargo run --quiet -- migrate`), so a project has one entry point for both code generation and runtime commands.

Built-ins (`new`, `dev`, `make:*`) are delivered milestone by milestone; this version only forwards.

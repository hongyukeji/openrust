# Contributing to OpenRust

OpenRust is in pre-alpha development. Before changing code, read [AGENTS.md](AGENTS.md) for the repository workflow and [.agents/project/conventions.md](.agents/project/conventions.md) for code and Git conventions. Check the relevant crate README and `.agents/project/` facts for current contracts.

## Build and verify

Install a Rust toolchain compatible with the workspace `rust-version`, then run:

```bash
cargo check --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
```

The five verification commands are `fmt`, `clippy`, `test`, `doc` and `deny`; `cargo check` is a quick build check. Data-related changes also require PostgreSQL and MySQL integration tests as described in [.agents/project/memory/testing.md](.agents/project/memory/testing.md).

## Commits

Use the repository [commit script](.agents/project/scripts/commit) with a conventional type and a Chinese subject:

```bash
AI_AGENT_SOURCE="<agent> <model>" ./.agents/project/scripts/commit <type> "<中文主题>" "<说明>" -- <pathspec>...
./.agents/project/scripts/commit validate HEAD
```

Keep one functional change per commit. See the Git section of [conventions.md](.agents/project/conventions.md) for accepted types and attribution rules.

## License

Contributions are dual licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option. Unless you explicitly state otherwise, an intentional contribution is submitted under those terms without additional conditions.

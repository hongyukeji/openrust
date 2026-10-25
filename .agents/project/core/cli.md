# openrust-cli

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：全局 `openrust` 命令当前事实。

- 二进制名 `openrust`，clap derive；当前只有 `external_subcommand` 转发：`openrust <cmd> [args]` → 当前目录执行 `cargo run --quiet -- <cmd> [args]`，退出码透传；目录无 `Cargo.toml` 时报错。
- `new` / `dev` / `make:*` 按里程碑加入（M1 起）。

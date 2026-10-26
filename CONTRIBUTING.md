# 参与 OpenRust 贡献

OpenRust 目前处于 pre-alpha 开发阶段。修改代码前，请阅读 [AGENTS.md](AGENTS.md) 了解仓库工作流程，阅读 [.agents/project/conventions.md](.agents/project/conventions.md) 了解代码与 Git 约定，并查看相关 crate README 和 `.agents/project/` 当前事实以确认契约。

## 构建与验证

安装与 workspace `rust-version` 兼容的 Rust 工具链，然后运行：

```bash
cargo check --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
```

五项主要门禁是 `fmt`、`clippy`、`test`、`doc` 和 `deny`；`cargo check` 用于快速检查构建。数据相关变更还必须按照 [.agents/project/memory/testing.md](.agents/project/memory/testing.md) 执行 PostgreSQL 和 MySQL 集成测试。

## 提交

使用仓库的[提交脚本](.agents/project/scripts/commit)，采用约定的提交类型和中文主题：

```bash
AI_AGENT_SOURCE="<agent> <model>" ./.agents/project/scripts/commit <type> "<中文主题>" "<说明>" -- <pathspec>...
./.agents/project/scripts/commit validate HEAD
```

每次提交只包含一个功能变更。允许的提交类型和署名规则见 [conventions.md](.agents/project/conventions.md) 中的 Git 章节。

## 许可证

Contributions are dual licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option. Unless you explicitly state otherwise, an intentional contribution is submitted under those terms without additional conditions.

# ADR-019 中性 crate 目录与聚合应用包名

- 状态：Partially Superseded（能力目录重命名由 [ADR-021](adr-021-loco-style-module-layout.md) 收敛为主框架包内模块；若仍保留的独立 package，目录继续采用中性名称。内部 `apps/server` package 命名不再实施，因为宿主将由 tests/examples 接替。）
- 日期：2026-10-25
- 关联：[ADR-006](adr-006-rust-native-naming.md)、[ADR-007](adr-007-aggregate-server-entry.md)

## 背景

`crates/openrust-cache` 这类目录同时重复品牌和能力名称，源码树浏览不够直观。聚合入口位于 `apps/server`，Cargo 包名却使用 `openrust-server`，与其内部、不可发布的应用角色不一致。Cargo 发布包身份与仓库源码目录承担不同职责，应分别命名。

## 决策

- 框架能力 crate 的源码目录按职责使用中性能力名，例如 `crates/core`、`crates/cache`、`crates/http`、`crates/auth`、`crates/db`、`crates/queue`、`crates/storage`、`crates/cli`、`crates/mail`、`crates/events`、`crates/macros`、`crates/test`。
- 可发布 Cargo 包名继续使用 `openrust-<capability>`，例如目录 `crates/cache` 中的包仍为 `openrust-cache`。源码目录重命名不改变 crates.io 身份、Rust crate 导入名、公共 API 或 feature 名称。
- 门面 crate 保留目录 `crates/openrust` 与包名 `openrust`。
- 聚合应用目录保留 `apps/server`，其私有 Cargo 包名与二进制名均为 `server`，并保持 `publish = false`。它继续只依赖 `openrust` 门面，不承载业务资源。workspace 的默认成员及根目录启动体验保持有效。
- 应用运行时显示名与 Cargo 包名分属不同配置；默认值使用中性名称 `server`，并同步更新相关快照、测试和文档。
- 迁移是一个独立、可审查的重命名变更；不与当前认证行为变更混合。先核对工作区和认证 WIP，再更新所有真实路径与宿主包/二进制引用。只在明确指代发布包或 Rust import 时保留 `openrust-*` 字样。

## 影响与迁移

- 更新 workspace member/path、依赖路径、脚本、CI、模板、测试环境变量名、文档中的文件路径及运行命令；检查 CLI 生成器对 crate 依赖的路径引用。
- 不提供旧目录或 `openrust-server` 的并行兼容别名；这些路径和内部二进制不属于已发布库 API。公开包名保持稳定。
- `apps/server` 默认运行和 `cargo run -p server`、包选择测试等入口须继续工作；更新 `CARGO_BIN_EXE_server` 等编译期二进制引用。

## 验收

- `cargo metadata --no-deps` 显示全部包名、manifest 路径、默认成员符合决策；`cargo run` 和 `cargo run -p server` 均可启动聚合应用。
- `rg` 检查没有遗留的 crate 旧目录路径或 `openrust-server` 内部包/二进制引用；公开包名和 Rust import 的 `openrust-*` 使用保持原样。
- 通过 `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo doc --workspace --no-deps`，并验证生成项目的 `cargo check` 与 `cargo test`。

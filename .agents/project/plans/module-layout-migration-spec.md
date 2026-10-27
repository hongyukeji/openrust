# 根模块与目录收尾 Spec

> 受众：AI Agent
> 状态：根宿主与工具目录迁移已实施。本轮按 [ADR-064](../decisions/adr-064-canonical-example-application.md) 收敛示例：唯一综合应用位于 `examples/example/`，独立 consumer 验收位于 `tests/consumers/`，不再使用根 `examples/` 存放用户示例。
> 决策：[ADR-062](../decisions/adr-062-root-server-entry.md)、[ADR-064](../decisions/adr-064-canonical-example-application.md)。工具包与源码交付迁移已完成，见 [ADR-021](../decisions/adr-021-loco-style-module-layout.md) 与 [ADR-039](../decisions/adr-039-source-workspace-distribution.md)。

## 目标

1. 根 `openrust` package 承载框架 library 与框架开发/集成用 `server` bin，二进制源码统一位于根 `framework/src/`。
2. 从仓库根目录直接执行 `cargo run` 启动宿主；根 package 是默认成员，`cargo test` 运行根 package 的集成测试；全 workspace 验证使用 `--workspace`。保留 `CARGO_BIN_EXE_server` 真实进程覆盖。
3. `apps/` 不保留框架自带的 `apps/server` package；根宿主显式业务模块注册后续由 [ADR-063](../decisions/adr-063-explicit-app-module-registry.md) 补充，生成应用仍由 `apps/<name>/` 承载。
4. 集成测试迁至根 `tests/server/`，配置放项目根 `config/`；Rust 配置代码仍放 `framework/src/config/`。性能基准集中在 `tools/performance/` workspace package；唯一综合用户示例位于 `examples/example/`。
5. 保留 `testing` 独立工具 package 的下游入口 `testing::TestDatabase`，内部实现统一在根 `framework/src/testing/`，避免根集成测试依赖 `testing` 造成 package 依赖环。
6. 同步 workspace、feature、CLI 工作区识别、CI、README、Agent 规则与当前事实；不丢失现有测试、feature 门控、子进程语义或未提交 WIP。

## 当前与目标

| 目标 | 路径/入口 |
| --- | --- |
| 框架 library | `framework/src/lib.rs` 与根 `framework/src/` modules |
| 集成宿主 | `src/main.rs`，Cargo bin 名称 `server`；仓库根 `cargo run` |
| 用户应用 | `apps/<name>/`，由生成器维护 workspace 与应用启动/配置边界 |
| 运行配置 | 根 `config/`；Rust 配置实现 `framework/src/config/` |
| 宿主集成测试 | 根 `tests/server/`，显式 `[[test]]` 声明、原 target 名和 feature 要求保持 |
| TestDatabase 实现/兼容入口 | `framework/src/testing/`；独立 `testing` package re-export `openrust::testing::TestDatabase` |
| 基准测试 | `tools/performance/benches/db_routing.rs`，`performance` package 的 `db-routing-bench` feature；数据库 feature 转发到根 `openrust` package |
| 工具边界 | `tools/cli/`、`tools/macros/`、`tools/testing/`、`tools/performance/` 为独立 package，分别承担命令、proc-macro、下游测试工具与性能验证 |

## 已实施变更

- 原 `apps/server/src/{main,app,routes}.rs`、配置、测试、基准迁至根目录目标。
- 根 package 添加显式 `server` bin 和宿主测试 target，workspace 移除 server package 成员及 workspace dependency。
- TestDatabase 实现迁入框架源码，工具 package 只保留兼容 re-export。
- CLI workspace 识别不再要求 `apps/server` package；`apps/` 的业务应用发现规则保持。
- 移除旧 `apps/server/Cargo.toml` 与 README；文档/CI/feature 配置切换至根 package。

## 验收

1. `cargo metadata --no-deps --format-version 1` 显示 workspace 含 `openrust`、`cli`、`macros`、`testing`、`performance`；root package 有 `server` bin 和集成测试 target，`performance` package 有数据库路由 bench target。
2. 根目录 `cargo run -- config show` 和裸 `APP_SERVER__PORT=43187 cargo run` 均成功；后者日志显示监听 `127.0.0.1:43187`，绕过本机代理请求真实 TCP `/health` 返回 200。
3. `cargo test --workspace`、默认与 all-features Clippy/tests/docs、fmt、no-default-features check 与 cargo deny 通过；CLI 全部 19 项测试通过，包括 `creates_multiple_workspace_apps_with_local_dependencies_and_isolated_configs` 与 `rejects_external_invalid_duplicate_and_existing_destinations_without_mutation`。
4. PostgreSQL/MySQL/Redis 专项入口使用现有适用服务并复验；不创建重复服务。保留 worker/scheduler 子进程、Keycloak fixture、真实隔离库和 `CARGO_BIN_EXE_server` 覆盖。
5. 扫描有效代码和文档中旧 `apps/server` 入口，历史 ADR/CHANGELOG 仅保留可识别的历史记录；不更改其他功能簇已有 WIP。

## 本次验收结果（2026-10-27）

- `cargo metadata --no-deps --format-version 1` 确认 workspace 有五个 package、根 package 提供 `server` bin，唯一默认成员为 `openrust`；`cargo run -- config show` 成功。
- 裸 `APP_SERVER__PORT=43187 cargo run` 实际启动根 `server`；`curl --noproxy '*' http://127.0.0.1:43187/health` 返回 HTTP 200，随后 SIGINT 正常退出。
- 迁移后的两项 CLI 生成器回归及 `cargo test -p cli`（19 项）通过；默认和 all-features workspace tests、Clippy、warnings-denied Rustdoc、`cargo check --workspace --no-default-features`、`cargo deny check` 均已通过。`cargo fmt --all --check` 与 `git diff --check` 本次复核。
- 本次只验证裸 `cargo run` 的端口覆盖和存活端点；PostgreSQL/MySQL/Redis 迁移前后专项不重复运行，采用已有适用服务的迁移验收记录。

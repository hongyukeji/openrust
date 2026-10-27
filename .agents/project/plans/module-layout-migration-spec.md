# 根模块与目录收尾 Spec

> 受众：AI Agent
> 状态：迁移已实施，最终验收待完成。真实 server bin、配置和集成测试入口已迁入根 package；工具 package 已收纳到 `tools/`。现有 CLI 测试记录显示 `creates_multiple_workspace_apps_with_local_dependencies_and_isolated_configs` 与 `rejects_external_invalid_duplicate_and_existing_destinations_without_mutation` 在生成项目时返回 `No such file or directory`；根因尚未确认，不能将本 Spec 标记为验收完成。
> 决策：[ADR-062](../decisions/adr-062-root-server-entry.md)。工具包与源码交付迁移已完成，见 [ADR-021](../decisions/adr-021-loco-style-module-layout.md) 与 [ADR-039](../decisions/adr-039-source-workspace-distribution.md)。

## 目标

1. 根 `openrust` package 承载框架 library 与框架开发/集成用 `server` bin，二进制源码统一位于根 `src/`。
2. 从仓库根目录直接执行 `cargo run` 启动宿主，`cargo test` 运行根 package 的集成测试；保留 `CARGO_BIN_EXE_server` 真实进程覆盖。
3. `apps/` 只作为开发者业务应用目录约定，不保留框架自带的 `apps/server` package。
4. 集成测试迁至根 `tests/server/`，配置放项目根 `config/`；Rust 配置代码仍放 `src/config/`。性能基准集中在 `tools/performance/` workspace package，避免把性能工具入口并入框架 package。
5. 保留 `testing` 独立工具 package 的下游入口 `testing::TestDatabase`，内部实现统一在根 `src/testing/`，避免根集成测试依赖 `testing` 造成 package 依赖环。
6. 同步 workspace、feature、CLI 工作区识别、CI、README、Agent 规则与当前事实；不丢失现有测试、feature 门控、子进程语义或未提交 WIP。

## 当前与目标

| 目标 | 路径/入口 |
| --- | --- |
| 框架 library | `src/lib.rs` 与根 `src/` modules |
| 集成宿主 | `src/main.rs`，Cargo bin 名称 `server`；仓库根 `cargo run` |
| 用户应用 | `apps/<name>/`，由生成器维护 workspace 与应用启动/配置边界 |
| 运行配置 | 根 `config/`；Rust 配置实现 `src/config/` |
| 宿主集成测试 | 根 `tests/server/`，显式 `[[test]]` 声明、原 target 名和 feature 要求保持 |
| TestDatabase 实现/兼容入口 | `src/testing/`；独立 `testing` package re-export `openrust::testing::TestDatabase` |
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
2. `cargo run -- config show` 从仓库根启动；默认 serve 经真实 TCP `/health` 返回成功；`APP_SERVER__PORT` 仍有效。
3. `cargo test --workspace`、all-features Clippy/tests/docs、fmt 与 cargo deny 通过；CLI fixture 与 CLI workspace 检测测试通过。
4. PostgreSQL/MySQL/Redis 专项入口使用现有适用服务并复验；不创建重复服务。保留 worker/scheduler 子进程、Keycloak fixture、真实隔离库和 `CARGO_BIN_EXE_server` 覆盖。
5. 扫描有效代码和文档中旧 `apps/server` 入口，历史 ADR/CHANGELOG 仅保留可识别的历史记录；不更改其他功能簇已有 WIP。

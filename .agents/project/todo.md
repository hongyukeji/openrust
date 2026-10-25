# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、crate 事实与 `memory/`。

## 前置（开发者）

- 安装 Rust stable 工具链（`rustup`），本机 `cargo --version` 可用；否则 M0 的代码功能簇无法验证。

## M0 决策与基线

1. **workspace 骨架与 lint 基线**：根 `Cargo.toml` 声明 workspace、`[workspace.package]`（edition 2024、`rust-version`、license、repository）、`[workspace.dependencies]` 与 `[workspace.lints]`（clippy all + pedantic，库 crate 拒绝 `unwrap_used` / `expect_used` / `panic` / `todo` / `unimplemented`）；创建 `crates/openrust`、`crates/openrust-core`、`crates/openrust-http`、`crates/openrust-cli` 空壳（`lib.rs` 带 crate 文档与 `#![forbid(unsafe_code)]` / `#![deny(missing_docs)]`）；`deny.toml`。验收：`cargo fmt --check`、`cargo clippy --workspace --all-targets -D warnings`、`cargo test --workspace`、`cargo deny check` 通过。
2. **CI**：`.github/workflows/ci.yml` 执行 fmt / clippy / test / doc / deny / MSRV 构建；预留 PostgreSQL 与 MySQL service 的双库 job（M2 启用）。验收：工作流文件通过 `actionlint`，本地等价命令通过。
3. **Spike A 双库迁移**：在 `spikes/dual-db-migration/` 用 SeaORM 2 + `sea-orm-migration` 对 PostgreSQL 与 MySQL 执行同一组迁移（含 `timestamptz` / `DATETIME(6)`、`jsonb` / `json`、唯一索引、外键），验证重复执行幂等与回滚。结论写入 ADR-002「验证」节。
4. **Spike B 数据库队列**：在 `spikes/db-queue/` 验证两库 `SELECT ... FOR UPDATE SKIP LOCKED` 拉取、重试计数、延迟任务；对比 `apalis` 的接入成本与抽象泄漏。结论写入 ADR-003，定稿自研或封装。
5. **Spike C 提取器组合**：在 `spikes/axum-extractors/` 验证 `State<T>` + `FromRef`、`Validated<Json<T>>`、`CurrentUser` 自定义提取器与 RFC 9457 错误响应可组合且错误信息可读。结论用于修订 [框架设计](plans/framework-design.md)「路由与控制器」与「错误模型」。

Spike 代码不进入 crate，M0 结束后删除目录，只保留 ADR 结论。

## 阻塞

无。

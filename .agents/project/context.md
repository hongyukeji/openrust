# 项目上下文

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：OpenRust 是全新的开源 Rust 应用开发框架，当前处于第一代设计完成、M0 尚未开始的阶段。

## 当前状态

| 项 | 值 |
| --- | --- |
| 版本 | 0.0.0（未发布，crates.io 名称未预留） |
| 阶段 | 第一代 M0「决策与基线」进行中：workspace、lint、CI、`openrust-core` / `openrust-http` / 门面 / CLI 转发已实现，Spike A / B 待做 |
| 仓库 | `https://github.com/hongyukeji/openrust`（远端尚未创建）；本地 `/Users/Home/Codes/Rust/openrust` |
| 协作模式 | 单开发者 + AI Agent；Claude 负责方案与设计，Codex 负责实现与验证 |
| 本机工具链 | Rust stable 1.94（`rustup`），MSRV 1.92；`cargo-deny` 未安装，由 CI 执行 |

## 固定约束

- OpenRust 不是任何既有项目的移植，没有兼容包袱；设计以 [第一代计划](plans/gen1-plan.md) 与 [框架设计](plans/framework-design.md) 为准。
- 应用内核自研，基于 `tokio` + `axum` + `tower`；Loco 只作参考与对标（[ADR-001](decisions/adr-001-own-kernel-on-axum.md)）。
- 第一代正式支持 PostgreSQL 14+ 与 MySQL 8.0+；每个数据功能必须有双库证据（[ADR-002](decisions/adr-002-seaorm-dual-database.md)）。
- Redis 不是必需依赖；数据库队列、进程内缓存、数据库会话为默认（[ADR-003](decisions/adr-003-database-queue-default.md)）。
- 认证默认为 Cookie 会话 + 个人访问令牌，JWT 不在第一代（[ADR-004](decisions/adr-004-session-and-token-auth.md)）。
- 许可证 MIT OR Apache-2.0；Edition 2024；MSRV 为 stable 减两个小版本（[ADR-005](decisions/adr-005-license-naming-msrv.md)）。
- 库 crate 禁止 `unsafe`、`unwrap` / `expect` / `panic`；公共项必须有文档。细则见 [conventions.md](conventions.md)。
- 功能范围以计划的「第一代能力范围」为准，不在里程碑内追加能力；新需求先进 `roadmap.md`。

## 对外动作（仅开发者执行）

- 创建 GitHub 仓库 `hongyukeji/openrust` 并推送 `main`。
- 在 crates.io 预留 `openrust`、`openrust-cli` 及 `openrust-*` 名称（发布 0.0.0 占位版本）。
- 发布任何版本到 crates.io。

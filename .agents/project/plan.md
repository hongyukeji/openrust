# 当前计划

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：第一代共 M0-M6 七个里程碑，当前在 M0。详细交付与验收见 [第一代计划](plans/gen1-plan.md)「里程碑」。

## 当前里程碑

M0 决策与基线。骨架、lint、CI、内核与 HTTP 层已落地；Spike A / B 已完成，结论见 ADR-002 / ADR-003。M1 的 HTTP 资源路由与响应类型已实现；其余 M1 任务见 `todo.md`。进入 M1 的条件：CI 全绿。

## 里程碑顺序

| 里程碑 | 主题 | 主要 crate |
| --- | --- | --- |
| M0 | 决策与基线 | workspace、CI、spikes |
| M1 | 内核与 HTTP | `openrust-core`、`openrust-http`、`openrust-cli`（`new` / `dev` / `serve`） |
| M2 | 数据层 | `openrust-db`、`openrust-test`、模板 `--db` |
| M3 | 邮件、认证与授权 | `openrust-mail`、`openrust-auth` |
| M4 | 队列、调度与事件 | `openrust-queue`、`openrust-events` |
| M5 | 缓存、存储与可观测性 | `openrust-cache`、`openrust-storage`、`openrust-http` 可观测性 |
| M6 | 开发者体验与 0.1 发布 | `docs/`、`examples/blog`、模板、生成器全集 |

## 路由

| 任务类型 | 入口 |
| --- | --- |
| 架构 / crate 边界 | [architecture.md](architecture.md)、[框架设计](plans/framework-design.md)、`decisions/` |
| 编码 / Git / 验证 | [conventions.md](conventions.md) |
| 依赖 / 版本 | [stack.md](stack.md) |
| 里程碑范围与验收 | [第一代计划](plans/gen1-plan.md) |
| 第一代之后 | [roadmap.md](roadmap.md) |

## 验证入口

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check
```

数据库相关 crate 另按 [memory/testing.md](memory/testing.md) 在 PostgreSQL 与 MySQL 上各跑一次。

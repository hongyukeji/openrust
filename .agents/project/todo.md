# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## M1 内核与 HTTP（进行中）

- **当前功能簇**：按 ADR-006 执行代码改名，后续为设计文档与元文件。
- `#[openrust::resource]` 宏从 `controllers/` 模块的 `list`/`get`/`create`/`update`/`delete` 动作函数生成 `impl Resource`（函数名避让 `axum::routing` 冲突的方案见 ADR-006 §4）。
- `openrust-cli`：`new <name> --db postgres|mysql`（M1 先生成无数据库模板，M2 加 `--db`）、`dev`、`serve` 代理；模板放 `templates/`，生成后 `cargo test` 通过的快照测试。
- 启用 CI `databases` job 的前提是 `openrust-db` 存在，归 M2。

## M2 数据层（M1 收口后领取）

- Redis 驱动落地时，将 `REDIS_URL` 映射到对应 Redis 连接配置；当前没有 Redis 配置段。

- 在最低支持版本 PostgreSQL 14 与 MySQL 8.0 上复测 ADR-002 / ADR-003 的迁移与 `SKIP LOCKED` 结论（spike 只测了 PG 15/18、MySQL 8.4）。
- `memory/testing.md` 引用的 `compose.ci.yaml` 不存在：随 `databases` job 一并补齐或改正引用。

## M3 邮件、认证与授权（M2 收口后领取）

- 确认 `CurrentUser` 能与 `Inject` 共存于同一 handler，在 `openrust-auth` 测试中覆盖。

## 阻塞

无。

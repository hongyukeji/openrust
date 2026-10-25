# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## M0 决策与基线

1. **Spike C 提取器组合**：已由 `openrust-http` 的 `Inject` / `Validated` 与 `tests/router.rs` 直接验证，不再单独做 spike；剩余项是确认 `CurrentUser`（M3）能与 `Inject` 共存于同一 handler，届时在 `openrust-auth` 测试中覆盖。

## M1 内核与 HTTP（M0 收口后领取）

- `Routes::resource::<C>(path)` 约定与 `Created<T>` / `NoContent` / `Paginated<T>` 响应类型，含测试。
- CORS、压缩层接入默认栈并由 `[server]` 配置开关。
- `openrust-cli`：`new <name> --db postgres|mysql`（M1 先生成无数据库模板，M2 加 `--db`）、`dev`、`serve` 代理；模板放 `templates/`，生成后 `cargo test` 通过的快照测试。
- 启用 CI `databases` job 的前提是 `openrust-db` 存在，归 M2。

## M2 数据层（M1 收口后领取）

- 在最低支持版本 PostgreSQL 14 与 MySQL 8.0 上复测 ADR-002 / ADR-003 的迁移与 `SKIP LOCKED` 结论（spike 只测了 PG 15/18、MySQL 8.4）。
- `memory/testing.md` 引用的 `compose.ci.yaml` 不存在：随 `databases` job 一并补齐或改正引用。

## 阻塞

无。

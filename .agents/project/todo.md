# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## M0 决策与基线

1. **Spike A 双库迁移**：在 `spikes/dual-db-migration/`（独立 Cargo 项目，不进 workspace）用 `sea-orm` 2 + `sea-orm-migration` 对 PostgreSQL 与 MySQL 执行同一组迁移（`timestamptz` / `DATETIME(6)`、`jsonb` / `json`、唯一索引、外键），验证重复执行幂等与回滚。结论写入 ADR-002「验证」节；完成后删除目录。
2. **Spike B 数据库队列**：在 `spikes/db-queue/` 验证两库 `SELECT … FOR UPDATE SKIP LOCKED` 拉取、重试计数、延迟任务；对比 `apalis` 的接入成本与抽象泄漏。结论写入 ADR-003，定稿自研或封装；完成后删除目录。
3. **Spike C 提取器组合**：已由 `openrust-http` 的 `Inject` / `Validated` 与 `tests/router.rs` 直接验证，不再单独做 spike；剩余项是确认 `CurrentUser`（M3）能与 `Inject` 共存于同一 handler，届时在 `openrust-auth` 测试中覆盖。

## M1 内核与 HTTP（M0 收口后领取）

- `Routes::resource::<C>(path)` 约定与 `Created<T>` / `NoContent` / `Paginated<T>` 响应类型，含测试。
- CORS、压缩层接入默认栈并由 `[server]` 配置开关。
- `openrust-cli`：`new <name> --db postgres|mysql`（M1 先生成无数据库模板，M2 加 `--db`）、`dev`、`serve` 代理；模板放 `templates/`，生成后 `cargo test` 通过的快照测试。
- 启用 CI `databases` job 的前提是 `openrust-db` 存在，归 M2。

## 阻塞

无。

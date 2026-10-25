# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## M1 内核与 HTTP（进行中）

- `#[openrust::resource]` 宏从控制器模块的 `index`/`show`/`store`/`update`/`destroy` 函数生成 `impl Resource`。
- CORS、压缩层接入默认栈并由 `[server]` 配置开关。
- `openrust-cli`：`new <name> --db postgres|mysql`（M1 先生成无数据库模板，M2 加 `--db`）、`dev`、`serve` 代理；模板放 `templates/`，生成后 `cargo test` 通过的快照测试。
- 启用 CI `databases` job 的前提是 `openrust-db` 存在，归 M2。

## M2 数据层（M1 收口后领取）

- 在最低支持版本 PostgreSQL 14 与 MySQL 8.0 上复测 ADR-002 / ADR-003 的迁移与 `SKIP LOCKED` 结论（spike 只测了 PG 15/18、MySQL 8.4）。
- `memory/testing.md` 引用的 `compose.ci.yaml` 不存在：随 `databases` job 一并补齐或改正引用。

## M3 邮件、认证与授权（M2 收口后领取）

- 确认 `CurrentUser` 能与 `Inject` 共存于同一 handler，在 `openrust-auth` 测试中覆盖。

## 阻塞

无。

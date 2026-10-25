# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## 后续里程碑

- OpenAPI：后续里程碑用 `utoipa` 生成 schema，并在非生产环境挂载 `/docs`（ADR-008 §6）。

## M2 数据层（进行中）

- 后续 M2 功能簇：seed、数据库测试隔离与 `openrust new --db` 模板。

## M3 邮件、认证与授权（M2 收口后领取）

- 确认 `CurrentUser` 能与 `Inject` 共存于同一 handler，在 `openrust-auth` 测试中覆盖。

## M4/M5 Redis 驱动（对应里程碑时领取）

- Redis 驱动建立配置段时，将 `REDIS_URL` 映射到连接配置，并保证同键的 `OPENRUST_` 变量优先。

## 阻塞

无。

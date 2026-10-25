# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## 后续里程碑

- OpenAPI：后续里程碑用 `utoipa` 生成 schema，并在非生产环境挂载 `/docs`（ADR-008 §6）。

## M3 邮件、认证与授权（进行中）

- `openrust-auth` 起步验收：确认 `CurrentUser` 能与 `Inject` 共存于同一 handler，并添加测试；随后依据 M3 contract 扩展认证、会话、授权与通信功能。

## M4/M5 Redis 驱动（对应里程碑时领取）

- Redis 驱动建立配置段时，将 `REDIS_URL` 映射到连接配置，并保证同键的 `OPENRUST_` 变量优先。

## 阻塞

无。

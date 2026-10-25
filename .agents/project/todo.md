# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## 后续里程碑

- OpenAPI：后续里程碑用 `utoipa` 生成 schema，并在非生产环境挂载 `/docs`（ADR-008 §6）。

## M3 邮件、认证与授权（进行中）

- 当前功能簇：实现 `openrust-mail` 的 `Email` / `Mailer` 与 log、fake、SMTP drivers，接入门面和 `apps/server` 并覆盖测试。
- 认证 API 进入实现前补充 ADR：定义业务自有用户类型如何映射到 `CurrentUser` / `OptionalUser`、路由认证配置与身份注入契约；现有 ADR-004 明确了安全策略但未定公共 API。

## M4/M5 Redis 驱动（对应里程碑时领取）

- Redis 驱动建立配置段时，将 `REDIS_URL` 映射到连接配置，并保证同键的 `OPENRUST_` 变量优先。

## 阻塞

无。

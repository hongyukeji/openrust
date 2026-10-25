# 活跃待办

> 受众：AI Agent
> 更新：2026-10-25
> 摘要：只放未完成且可领取的功能簇。完成即删除，证据进 commit、`core/<crate>.md` 与 `memory/`。

## 后续里程碑

- OpenAPI：后续里程碑用 `utoipa` 生成 schema，并在非生产环境挂载 `/docs`（ADR-008 §6）。

## M3 邮件、认证与授权（进行中）

- 当前功能簇：按 [ADR-012](decisions/adr-012-auth-current-user-contract.md) 实现业务用户 ID 映射、路由认证配置和身份注入，并覆盖共享 Session/PAT 撤销、CSRF 与 `Inject` 共存。
- 本簇验收前按 [执行校准](plan.md#执行校准) 核对契约：共享会话与跨节点撤销、认证所需全局限流/验证码边界、注册/登录公开扩展点及 `Inject` 共存；后续 M3 任务按完整 v1 覆盖清单逐簇领取，不以身份注入完成代替整个 M3。
- ADR-012 已定义身份/路由 contract；本簇后续仍需形成独立授权 API 与角色/权限变更传播 contract，不把身份提取实现当作 M3 完成。

## M5 缓存驱动与 store（M5 领取）

- 按 [ADR-014](decisions/adr-014-cache-driver-and-store-contract.md) 实现 `openrust-cache` 与 `CachePlugin`：memory 默认、file、feature 可选 Redis；default/named stores、custom driver 注册、统一序列化/TTL/错误与原子能力边界。
- 默认生成项目无需 Redis；Redis 配置读取 `REDIS_URL`，同键 `OPENRUST_` 环境变量优先。共享缓存用途在集群配置中必须选择可用共享 driver，禁止静默退化。
- 验收：统一基础契约测试；file 临时目录安全路径、过期、原子替换、并发及 I/O 错误；Redis 真实集成；默认/file/redis feature 组合；`apps/server` 接入；独立公开扩展示例与 README/core 事实。

## 阻塞

无。

# M3 HTTP 路由限流 Spec

- 目标：让普通业务路由可复用已验收的 `RateLimiter`，并由框架提供安全的常用身份 key 策略。
- 公开 API：在 `openrust::http` 暴露 Tower route layer 与策略构造器，应用按路由显式启用。
- 身份：支持可信代理解析后的 IP、已认证用户 ID、用户缺席时回退 IP，以及应用提供的稳定自定义 key。
- 隔离：每个策略要求稳定 scope；框架组合 scope 与身份，敏感原始凭据不进入限流 key。
- 顺序：身份策略要求认证时，AuthLayer 必须先在请求方向执行；匿名回退不要求认证层。
- 失败：超限复用统一 429 与 `Retry-After`；无效策略、缺失必要身份和 store 故障均拒绝请求，不静默放行。
- 代理：IP 策略只用 `ClientIp`，任意 `X-Forwarded-For` 不可信；沿用 `server.trusted_proxy_ranges`。
- 覆盖：只限被 route layer 包裹的路由；不自动限 `/health`、fallback 或未包裹路由。
- 验收：行为测试覆盖 key 隔离、认证顺序、匿名回退、伪造代理、429、存储错误；PostgreSQL/MySQL 复用 store 集成用例确认并发共享。
- 排除：不新增计数算法/store、不实现 ORM 查询/DTO 能力；用户数据边界作为独立簇审计。

# ADR-015 标准应用 Redis 默认缓存校准

- 状态：Accepted（规划与公共 contract；缓存实现待 M5）
- 日期：2026-10-25
- 替代：ADR-014 中 memory 作为标准生成应用默认缓存的决定；恢复 ADR-013 的 Redis 默认方向。ADR-014 其余驱动/store、能力检查与错误语义继续有效。

## 背景

开发者已明确要求标准应用默认 Redis，并同时支持单机与集群。无需外部服务的默认缓存建议不能覆盖这一要求；默认应用配置与库的 Cargo 默认 feature 是不同层次。

## 决策

- 标准生成应用显式启用 `cache-redis` 并将 default store 配置为 Redis。`openrust dev` 按项目配置启动数据库、Redis、Mailpit 等依赖，用户也可提供已有服务。
- 库/门面的默认 feature 保持可组合，不强制所有使用者编译 Redis；memory/file 是显式可选驱动。无数据库/最小 HTTP 模板不因本决定强制安装缓存。
- 安装 `CachePlugin` 却缺少 `[cache]` 或 default store 配置时，明确报告配置错误，不隐式创建 memory store。标准模板负责提供可用的 Redis 配置；缺失 feature/URL 或服务不可达明确报告，不静默降级。
- 单机同样可使用 Redis；集群依赖的共享/原子语义仍按目标 store 能力与真实服务验证。Redis 默认不等于全部功能自动支持集群，也不要求 Redis Cluster。
- 队列与 Session 继续默认共享数据库；不因缓存默认值改变而替换其后端。具体驱动、命名 store、TTL、错误和扩展规则沿用 ADR-014。
- 其他 Agent 不得以“最小依赖”建议自行改回 memory 默认；若需调整方向先取得开发者明确决定，并同步全部权威入口。

## 验收与影响

M5 验证标准 Redis 默认路径、显式 memory/file、缺失配置/feature/服务的错误、named stores 和真实共享语义。当前只校准文档；不宣称 Redis driver、生成配置或开发编排已经实现，不修改运行中的服务和凭据。

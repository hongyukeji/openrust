# ADR-060：密码哈希的有界并发

> 状态：Accepted
> 日期：2026-10-27

## 背景

Argon2 每次操作有明确的成本参数，但直接提交 `spawn_blocking` 会让并发请求无限进入 Tokio 阻塞池的等待队列。登录限流不能约束注册、重置、验证码或应用直接调用哈希服务的总资源。

## 决策

- `Passwords` 每个服务实例使用共享 Semaphore，`Clone` 共享同一并发预算；默认最多并行 2 个操作，可显式设置 1..=4。
- 容量耗尽时立即返回稳定错误 `password_capacity_exhausted`，内部语义状态为 HTTP 503；`semantic` HTTP 状态模式返回 503，默认 `always_ok` 模式仍按 ADR-008 返回统一 JSON 状态。无等待队列，不让过载请求继续占用内存等待。
- Semaphore permit 随 `spawn_blocking` 闭包移动，直到哈希/验证实际结束才释放。调用方 future 被取消不会提前归还正在工作的 CPU/内存预算。
- `AuthPlugin` 的认证与验证码服务使用同一个 `Passwords` 实例；应用自建多个独立实例时容量分别计算，跨进程节点也分别计算。部署者需按 Argon2 单次 memory 成本和进程内存预算设置容量。
- PHC 输入接受的单次 memory 最大为 262144 KiB；并发硬上限 4，因此单个服务实例的 Argon2 工作内存最高约 1 GiB，默认 2 个操作上限为 512 MiB。此数值不包含 Tokio、数据库、请求正文及进程其他开销。

## 影响与边界

- 新增 API：`AuthPlugin::with_password_operation_capacity` 与 `Passwords::with_max_concurrent_operations`。原 `PasswordConfig` 结构不变，避免破坏已有 struct literal。
- 只有使用相应 `Passwords` 实例执行的 Argon2 操作受限；其他自建 Argon2 或第三方密码服务不受控制。
- 503 是应用级容量保护，不替代按账户/IP 的限流。无重试等待保证；客户端可按自身策略退避。
- 配置容量非法时在插件构建/服务创建阶段失败，不 panic。

## 验收

- 单测证明容量满时 hash 与 verify 入口立即返回稳定错误，释放容量后可以恢复；克隆实例共享预算。
- AuthPlugin 测试证明配置错误在启动构建阶段失败，默认认证和验证码仍共享同一预算。
- 完成 workspace 源码门禁，并在 PostgreSQL 18/MySQL 8.4 验证已有认证流程回归；不声称该子簇完成生产登录负载、指标或全局 readiness 验收。

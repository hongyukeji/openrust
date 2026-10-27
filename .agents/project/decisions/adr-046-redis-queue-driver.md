# ADR-046 Redis Queue driver

- 状态：Accepted（已实施并完成 Redis 8.10.2 真实服务验收；最低版本与完整集群门禁仍待完成）
- 日期：2026-10-26
- 依据：[ADR-003](adr-003-database-queue-default.md)、[ADR-037](adr-037-queue-worker-lifecycle.md)、[ADR-043](adr-043-queue-driver-job-execution.md)

## 背景

数据库队列保持默认并满足事务派发。应用可选择 Redis 作为共享队列存储，以便多个 API/worker 实例共享任务；Redis 与业务数据库不共享事务，且 Redis 故障切换/持久性受部署配置影响，不能沿用数据库原子性保证。

## 决策

- 新增可选 `queue-redis` feature，复用 workspace 已有 `redis` 1.8.0、Rustls 与连接管理器，不增加队列库。默认仍使用 PostgreSQL/MySQL 数据库队列；只有应用显式选择 `queue.driver = "redis"` 并提供 Redis 连接变量或 URL 与 namespace 时才连接 Redis，feature/配置缺失或服务不可达均启动失败，不回退到数据库或内存。
- `QueuePlugin` 使用同一注册 handler、应用服务和 `Queue` facade 创建所选 driver。Redis 连接复用 `REDIS_HOST` / `REDIS_USERNAME` / `REDIS_PASSWORD` / `REDIS_PORT` / `REDIS_DB`；缓存和队列共用连接设置，namespace 是必填的隔离前缀。Redis driver 懒连接，使同步 Plugin build 不进行网络阻塞；服务不可达时派发或 worker 操作显式失败，不回退到其他 driver。Redis-only QueuePlugin 可不注册 `DatabasePlugin`；若存在 schedule 定义则 build 明确失败，因为当前 Scheduler slot 与派发共享数据库事务。Redis 是队列数据源时，事务派发返回 `queue_transaction_unsupported`；不得先写 Redis 再声称随 SQL 事务提交/回滚。
- Redis 保存待执行任务、活动唯一键、租约及失败任务。所有变更由 Lua 原子脚本完成，时间取 Redis `TIME`。同一 namespace 的脚本键使用同一个 hash slot；v1 支持普通 Redis endpoint，不宣称 Redis Cluster 分片兼容。多个 API/worker 必须连到同一逻辑 Redis 服务与 namespace。
- 派发成功表示 Redis 接受任务，不代表 Redis 磁盘落盘、复制到副本或业务副作用完成。领取时递增 attempt 并分配新的 owner token/lease；handler 成功后只允许当前 token 确认。失败和过期 lease 按有界指数退避重试，耗尽后保存在失败集合；人工重试保留原失败记录。成功确认前 worker 崩溃或确认响应丢失可能导致重复执行，保证为至少一次，handler 必须幂等；不保证任务恰好一次或 Redis 故障切换后不丢已确认写入。
- `dispatch_unique` 在任务待执行或执行期间按 `(queue, unique_key)` 返回同一个活动任务 ID；完成或归入失败集合后释放唯一键。它不是永久请求幂等记录。Redis 原子脚本支持同一 Redis namespace 内唯一性，不覆盖外部业务事务。
- handler 执行复用 `JobRegistry` / `JobContext`。进度和状态更新需匹配当前 lease token；timeout、取消、SIGTERM、资源 drain 与 worker join 遵循 ADR-037。Redis 不可用时 worker 返回错误，不确认任务；租约到期后由后续 worker 恢复。外部副作用提交后遇错仍可能重试。
- Redis driver 显式拒绝数据库事务派发和任务组；支持普通/延迟/唯一派发、单项运行、进度、失败保留与人工重试。Redis 队列与当前数据库事务型 scheduler slot 无法原子提交，故 Redis driver 配置不得注册 schedule 定义，并在应用 build 阶段以可诊断错误拒绝。
- Redis `maxmemory` eviction、持久化、备份、复制与故障切换由运维配置决定。部署须使用不驱逐队列键的策略和符合恢复目标的持久化/备份；框架不在客户端伪造这些保障。失败记录无自动 TTL，调用方须按运维策略归档/清理。

## 影响

标准应用仍以数据库队列开箱即用；需要共享队列时可显式选 Redis。数据库队列事务派发、任务组和双库语义不变。

## 验收记录

真实 Redis 8.10.2 服务验收：`cargo test -p openrust --features queue-redis --test queue_redis_driver` 的 3 项通过，覆盖四个独立连接 worker 竞争 40 项任务、唯一派发、重试/失败保留/幂等人工重试/进度、真实子进程强杀后的租约接管，以及错误 namespace 和不可达服务失败关闭。`cargo test -p openrust --features queue-redis --test queue_redis` 的 2 项通过，覆盖 Redis-only QueuePlugin 无 DatabasePlugin 接入及数据库 schedule 配置拒绝。另以 `examples/queue` 启动 Redis-only API，真实 TCP `POST /jobs` 返回任务 ID，再启动独立 worker 连接相同 namespace；API 和 worker 均正常启动并经 Ctrl-C 退出。

fmt、默认/all-features workspace clippy、默认/all-features workspace tests、warnings-denied 默认/all-features docs、`cargo deny check` 与 no-default-features check 通过。deny 保留既有 duplicate dependency 与 unmatched MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。Redis Queue 不访问 SQL，因此本簇不新增 PostgreSQL/MySQL 证据；数据库 QueueDriver 原有双库测试未被替代。Redis Cluster 分片、Redis 6.2 最低版本和生产多节点负载留待 v1 发布/集群门禁。

# ADR-003 数据库队列为默认，Redis 可选

- 状态：Partially Superseded（数据库队列默认与自研方向仍有效；缓存默认见 [ADR-015](adr-015-redis-default-calibration.md)，schema、公开 API 与 worker/scheduler 运行语义由 [ADR-037](adr-037-queue-worker-lifecycle.md) 定稿）
- 日期：2026-10-25

## 背景

开箱即用要求 `openrust dev` 不引入非必要的基础设施。PostgreSQL 与 MySQL 的当前目标稳定版本线支持 `SELECT ... FOR UPDATE SKIP LOCKED`，足以实现可靠的单表队列。

## 决策

- 队列使用数据库 `jobs` / `failed_jobs` 表，会话使用数据库 store；标准应用缓存默认策略由 ADR-015 定为 Redis。
- Redis 队列与 session store 是待补可选适配，不能从底层 Redis crate 推断已支持；当前队列实现范围以 ADR-037 为准。标准应用缓存默认 Redis，最小无缓存应用不强制安装缓存，库 feature 可组合；详见 ADR-013/014/015。
- 队列引擎**自研**（拉取、重试退避、延迟、失败表、优雅停机），不封装 `apalis`；依据见「验证」。

## 影响

- 调度器按 ADR-037 使用数据库触发槽唯一性与事务内派发去重，不把投递去重描述为外部副作用恰好一次。
- 事件监听器的 `queued` 模式复用同一队列。

## 验证

以下为历史 spike 证据，不等同于现有框架全量验收或当前 schema/API。正式实现与复验入口见 [队列事实](../core/queue.md) 和 ADR-037；不得照旧 schema 建立第二套队列。

本节中的“第一代范围之外”记录当时选型，不限制后续已批准范围。cron/IANA 时区、DST 与有界错过触发处理已列为 V1 补强，当前调度契约以 [ADR-037](adr-037-queue-worker-lifecycle.md) 和 [补齐计划](../plans/v1-foundation-completion.md) 为准；此变化不推翻数据库队列选型。

M0 Spike B（2026-10-25，代码已删除，结论以本节为准）。环境：Docker 临时容器，PostgreSQL 18.4 与 15.19、MySQL 8.4.11（本机无 8.0.x 镜像，8.0 起语法一致）；`sqlx` 0.9.0 运行时查询，8 个 worker、32 连接池。

| 场景 | PostgreSQL 18 / 15 | MySQL 8.4 |
| --- | --- | --- |
| 持锁演示：事务 A 持有第 1 行 `FOR UPDATE` 未提交，B 执行 `SKIP LOCKED` | 拿到第 2 行；`NOWAIT` 报 `could not obtain lock on row` | 拿到第 2 行；`NOWAIT` 报 3572 |
| 并发领取：8 worker 抢 400 条 | 总领取 400、唯一 400、无遗漏，约 150 ms | 总领取 400、唯一 400、无遗漏，约 290 ms |
| 重试：`max_attempts=3` 始终失败，退避 0.5 s | attempt 1、2 释放并退避重新入队（退避期内领取不到），第 3 次搬入 `failed_jobs` | 同左 |
| 延迟：`available_at = now + 1.5s` | 立即领取为空，约 1.55 s 后可领取（轮询间隔 100 ms） | 约 1.59 s |

MySQL 5.7.44 执行 `FOR UPDATE SKIP LOCKED` 报 1064 语法错误，确认 8.0+ 为硬下限。

验证中确认的双库差异（当前 `openrust::queue` 仍须按方言分支并有双库测试）：

- PostgreSQL 可单语句 `UPDATE … WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED) RETURNING *`；MySQL 无 `RETURNING`，必须在事务内先 `SELECT … FOR UPDATE SKIP LOCKED` 再 `UPDATE`。两库统一使用事务内两步写法即可，PostgreSQL 单语句作为后续优化。
- 时间一律取数据库时钟（PostgreSQL `now()`，MySQL `UTC_TIMESTAMP(6)`）计算 `available_at` / `reserved_at`，避免多副本时钟偏差；MySQL 列默认值需用表达式写法 `DEFAULT (UTC_TIMESTAMP(6))`。
- `sqlx` 0.9 拒绝未标注的动态 SQL 字符串，拼接方言片段必须经 `AssertSqlSafe` 显式包装（仅限框架内部、不含用户输入的片段）。
- `openrust::db` 的 MySQL driver 强制 TLS（Rustls），集成测试验证会话已协商 TLS。不得启用 `mysql-rsa`：其 `rsa 0.10.0-rc.19` 触发 RUSTSEC-2023-0071，且当前没有安全升级；不支持非 TLS MySQL 连接。

### `apalis` 评估

依据：`apalis` 1.0.0、`apalis-postgres` 1.0.0、`apalis-mysql` 1.0.0-rc.8 源码阅读，加 PostgreSQL 18 上的最小接入样例（`PostgresStorage::setup` + 20 次 `push` + 单 worker，20 条全部执行，状态 `Done`）。

- MySQL 存储仍是 release candidate，且依赖 `sqlx` 0.8.6，与框架的 `sqlx` 0.9（SeaORM 2）并存两个版本，两库连接池类型不兼容、无法共享，编译体积与审计面翻倍；PostgreSQL 存储用 `sqlx` 0.9。
- `apalis-postgres` 使用 `query_file!` 编译期宏：构建环境若设置了指向未迁移库的 `DATABASE_URL`（SeaORM CLI 的常见做法）则编译失败，需要 `SQLX_OFFLINE=true`，是下游使用者会踩的隐性约束。
- Schema 由 apalis 自带 `sqlx::migrate` 管理：PostgreSQL 建 `apalis` schema、`_sqlx_migrations` 表、`hstore` 扩展、`pg_notify` 触发器与存储函数；MySQL 单独建 `jobs` / `workers`。与 `sea-orm-migration` 形成第二套迁移体系，且 `CREATE EXTENSION` 需要额外权限；两库表结构不同（PostgreSQL 的 `job` 为 `bytea`、`metadata` 为 `hstore`，MySQL 为 `JSON` / `TIMESTAMP`），不符合“框架表不使用单库特性”的约定。
- 任务状态（`Pending` / `Queued` / `Running` / `Done` / `Failed` / `Killed`）与失败记录都在同一张表，没有 `failed_jobs`；重试退避、超时等通过 tower `Layer` 与 apalis 自己的 `Task` / `Backend` 抽象组合，`Job` 需要映射为 apalis 的函数式 handler，并让 apalis 的 `WorkerContext`、`TaskBuilder` 进入框架公共 API 边界，抽象泄漏明显。
- 能力上 apalis 提供的优先级、cron、工作流、LISTEN/NOTIFY、看板是第一代范围之外的；第一代需要的拉取、重试、延迟、失败表、停机，自研的核心 SQL 与事务逻辑在 spike 中约百行量级（见上文验证），可完全控制双库行为。

结论：接入成本（双 `sqlx` 版本、第二套迁移、编译期宏约束、RC 存储）高于自研收益，自研。若后续需要工作流等能力，另写 ADR 评估，不阻塞第一代。

## 当前持久化契约

旧 spike schema 已由 ADR-037 及实际 QueueMigration 替代，不再保留第二份建表建议。当前 job ID、lease/token、唯一键、任务组及 schedule slot 以源码为准；迁移通过应用 migrator 显式执行。大表 EXPLAIN/索引与负载基准仍需补齐，不能沿用小规模 spike 耗时作为性能结论。

## 对 `openrust::queue` 的约束

- 公共入口是 `Job`、`Queue` facade 与 `QueueDriver` 扩展 trait；`DatabaseQueue` 是当前默认 driver。可选 Redis driver 后续复用同一契约，不泄漏第三方队列库执行模型。
- 数据库 driver 复用 `openrust::db::Db` 连接池与迁移体系。方言差异集中在明确模块，参数绑定；静态 SQL 片段不能拼入用户输入。当前 SeaORM 路径不要求沿用历史 SQLx spike 的 `AssertSqlSafe` 调用。
- 至少一次执行、数据库时钟、owner/token fencing、超时重试与崩溃恢复按 ADR-037；代码/测试事实以 `core/queue.md` 为准。
- worker/scheduler supervisor 负责有界取消/drain/abort/join；数据库不可用时由 lease 到期恢复，不能承诺取消 future 会撤回外部副作用。
- 调度和 queued listener 复用队列；通用锁与调度 slot 的不同保证须明确，不把内部去重冒充完整公开锁能力。

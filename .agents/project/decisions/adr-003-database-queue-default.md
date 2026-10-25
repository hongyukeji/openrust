# ADR-003 数据库队列为默认，Redis 可选

- 状态：Accepted
- 日期：2026-10-25

## 背景

开箱即用要求 `openrust dev` 不引入非必要的基础设施。PostgreSQL 14+ 与 MySQL 8.0+ 都支持 `SELECT ... FOR UPDATE SKIP LOCKED`，足以实现可靠的单表队列。

## 决策

- 队列、会话、缓存默认不依赖 Redis：队列使用数据库 `jobs` / `failed_jobs` 表，会话使用数据库 store，缓存使用进程内 `moka`。
- Redis 作为 `queue-redis` / `cache-redis` feature 与 session store 选项提供，用于多实例共享与高吞吐场景。
- 队列引擎**自研**（拉取、重试退避、延迟、失败表、优雅停机），不封装 `apalis`；依据见「验证」。

## 影响

- 调度器多副本去重同样用数据库锁实现。
- 事件监听器的 `queued` 模式复用同一队列。

## 验证

M0 Spike B（2026-10-25，代码已删除，结论以本节为准）。环境：Docker 临时容器，PostgreSQL 18.4 与 15.19、MySQL 8.4.11（本机无 8.0.x 镜像，8.0 起语法一致）；`sqlx` 0.9.0 运行时查询，8 个 worker、32 连接池。

| 场景 | PostgreSQL 18 / 15 | MySQL 8.4 |
| --- | --- | --- |
| 持锁演示：事务 A 持有第 1 行 `FOR UPDATE` 未提交，B 执行 `SKIP LOCKED` | 拿到第 2 行；`NOWAIT` 报 `could not obtain lock on row` | 拿到第 2 行；`NOWAIT` 报 3572 |
| 并发领取：8 worker 抢 400 条 | 总领取 400、唯一 400、无遗漏，约 150 ms | 总领取 400、唯一 400、无遗漏，约 290 ms |
| 重试：`max_attempts=3` 始终失败，退避 0.5 s | attempt 1、2 释放并退避重新入队（退避期内领取不到），第 3 次搬入 `failed_jobs` | 同左 |
| 延迟：`available_at = now + 1.5s` | 立即领取为空，约 1.55 s 后可领取（轮询间隔 100 ms） | 约 1.59 s |

MySQL 5.7.44 执行 `FOR UPDATE SKIP LOCKED` 报 1064 语法错误，确认 8.0+ 为硬下限。

验证中确认的双库差异（`openrust-queue` 必须按方言分支并有双库测试）：

- PostgreSQL 可单语句 `UPDATE … WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED) RETURNING *`；MySQL 无 `RETURNING`，必须在事务内先 `SELECT … FOR UPDATE SKIP LOCKED` 再 `UPDATE`。两库统一使用事务内两步写法即可，PostgreSQL 单语句作为后续优化。
- 时间一律取数据库时钟（PostgreSQL `now()`，MySQL `UTC_TIMESTAMP(6)`）计算 `available_at` / `reserved_at`，避免多副本时钟偏差；MySQL 列默认值需用表达式写法 `DEFAULT (UTC_TIMESTAMP(6))`。
- `sqlx` 0.9 拒绝未标注的动态 SQL 字符串，拼接方言片段必须经 `AssertSqlSafe` 显式包装（仅限框架内部、不含用户输入的片段）。
- `sqlx` 0.9 连接 MySQL 8 默认认证（`caching_sha2_password`）在非 TLS 下需启用 `mysql-rsa` feature，否则连接失败；`openrust-db` 的 feature 设计需覆盖。

### `apalis` 评估

依据：`apalis` 1.0.0、`apalis-postgres` 1.0.0、`apalis-mysql` 1.0.0-rc.8 源码阅读，加 PostgreSQL 18 上的最小接入样例（`PostgresStorage::setup` + 20 次 `push` + 单 worker，20 条全部执行，状态 `Done`）。

- MySQL 存储仍是 release candidate，且依赖 `sqlx` 0.8.6，与框架的 `sqlx` 0.9（SeaORM 2）并存两个版本，两库连接池类型不兼容、无法共享，编译体积与审计面翻倍；PostgreSQL 存储用 `sqlx` 0.9。
- `apalis-postgres` 使用 `query_file!` 编译期宏：构建环境若设置了指向未迁移库的 `DATABASE_URL`（SeaORM CLI 的常见做法）则编译失败，需要 `SQLX_OFFLINE=true`，是下游使用者会踩的隐性约束。
- Schema 由 apalis 自带 `sqlx::migrate` 管理：PostgreSQL 建 `apalis` schema、`_sqlx_migrations` 表、`hstore` 扩展、`pg_notify` 触发器与存储函数；MySQL 单独建 `jobs` / `workers`。与 `sea-orm-migration` 形成第二套迁移体系，且 `CREATE EXTENSION` 需要额外权限；两库表结构不同（PostgreSQL 的 `job` 为 `bytea`、`metadata` 为 `hstore`，MySQL 为 `JSON` / `TIMESTAMP`），不符合“框架表不使用单库特性”的约定。
- 任务状态（`Pending` / `Queued` / `Running` / `Done` / `Failed` / `Killed`）与失败记录都在同一张表，没有 `failed_jobs`；重试退避、超时等通过 tower `Layer` 与 apalis 自己的 `Task` / `Backend` 抽象组合，`Job` 需要映射为 apalis 的函数式 handler，并让 apalis 的 `WorkerContext`、`TaskBuilder` 进入框架公共 API 边界，抽象泄漏明显。
- 能力上 apalis 提供的优先级、cron、工作流、LISTEN/NOTIFY、看板是第一代范围之外的；第一代需要的拉取、重试、延迟、失败表、停机，自研的核心 SQL 与事务逻辑在 spike 中约百行量级（见上文验证），可完全控制双库行为。

结论：接入成本（双 `sqlx` 版本、第二套迁移、编译期宏约束、RC 存储）高于自研收益，自研。若后续需要工作流等能力，另写 ADR 评估，不阻塞第一代。

## 表结构建议

`openrust-queue` 通过 `sea-orm-migration` 提供迁移（只用可移植类型，主键 `id BIGINT` 自增，时间 UTC：PostgreSQL `timestamptz`、MySQL `DATETIME(6)`）：

```text
jobs
  id            BIGINT PK auto
  queue         VARCHAR(64)  NOT NULL            -- 队列名，默认 "default"
  payload       JSON(B)      NOT NULL            -- 含 job 类型标识与序列化参数
  attempts      INT          NOT NULL DEFAULT 0  -- 每次领取 +1
  max_attempts  INT          NOT NULL DEFAULT 3
  available_at  timestamp    NOT NULL            -- 延迟与退避都写这里
  reserved_at   timestamp    NULL                -- 非空表示已被某 worker 领取
  reserved_by   VARCHAR(64)  NULL                -- worker 标识，便于排障
  created_at    timestamp    NOT NULL
  INDEX (queue, reserved_at, available_at, id)

failed_jobs
  id BIGINT PK auto, job_id BIGINT, queue VARCHAR(64), payload JSON(B),
  attempts INT, error TEXT, failed_at timestamp
```

- 领取：事务内 `SELECT id … WHERE queue=? AND reserved_at IS NULL AND available_at <= now ORDER BY available_at, id LIMIT n FOR UPDATE SKIP LOCKED`，随后 `UPDATE … SET reserved_at=now, reserved_by=?, attempts=attempts+1`，提交后再执行 job。
- 成功：删除行；失败且 `attempts < max_attempts`：清空 `reserved_at`，`available_at = now + backoff(attempts)`；否则同一事务内写 `failed_jobs` 并删除原行。
- 索引形态（含 `reserved_at IS NULL` 过滤与排序）在两库的执行计划与大表性能未在 spike 中压测，M4 实现时补 `EXPLAIN` 与基准。

## 对 `openrust-queue` 的约束

- 面向调用方的是 `trait Job` + `Queue` driver trait；`DatabaseQueue` 是默认 driver，`RedisQueue`（`queue-redis`）后续实现同一 trait；任何第三方队列库类型不得出现在公共 API。
- 数据库 driver 复用 `openrust-db` 的连接池，不另建 `sqlx` 连接池与迁移体系；方言分支集中在 `openrust-db::dialect` 或 `openrust-queue` 的单一模块，每个分支有双库测试（并发无重复领取、重试与失败表、延迟任务至少覆盖）。
- 时间只用数据库时钟；动态 SQL 片段只来自框架常量并经 `AssertSqlSafe` 包装。
- 至少一次（at-least-once）投递：job 必须幂等，文档明示。领取后 worker 崩溃会使行长期 `reserved_at` 非空，需要“可见性超时”回收（`reserved_at < now - timeout` 视为可重新领取，并计入 `attempts`）；该机制 spike 未验证，M4 必须实现并有测试。
- 优雅停机：停止领取新任务，等待在途任务到期，超时未完成的任务保持已领取状态交给可见性超时回收。
- 调度器多副本去重与 `queued` 事件监听器复用同一 `jobs` 表与领取逻辑，不引入第二套锁机制。

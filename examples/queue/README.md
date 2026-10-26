# 数据库队列组合示例

此示例用同一个应用二进制分别启动 API、worker 和 scheduler，数据库使用应用配置中的 `DATABASE_URL`。队列 migration 由应用的 `Migrator` 显式注册。

启用 PostgreSQL 或 MySQL feature 后，先迁移数据库，再分别运行以下进程：

```sh
cargo run -p openrust --example queue-demo --features queue,db-postgres -- migrate up
cargo run -p openrust --example queue-demo --features queue,db-postgres -- serve
cargo run -p openrust --example queue-demo --features queue,db-postgres -- queue work demo
cargo run -p openrust --example queue-demo --features queue,db-postgres -- schedule run
```

MySQL 将 `db-postgres` 换成 `db-mysql`。向 `POST /jobs` 发送请求会写入任务；worker 处理该任务。scheduler 每分钟派发一条 heartbeat。API 不会自动启动 worker 或 scheduler，生产环境应将这些命令作为独立进程管理。

## Redis 队列

应用可启用 `queue-redis` 并显式选择 Redis 队列驱动；无需 `QueueMigration`，也无需为队列本身配置数据库。Redis 队列与数据库 scheduler 不能共享原子事务，使用 Redis 时不要注册 schedule 定义。

```toml
[queue]
driver = "redis"
redis_namespace = "billing"
# 建议通过 APP_QUEUE__REDIS_URL 环境变量提供，不写入受版本控制的配置文件。
```

```sh
APP_QUEUE__DRIVER=redis \
APP_QUEUE__REDIS_NAMESPACE=billing \
APP_QUEUE__REDIS_URL=redis://127.0.0.1:6379/ \
cargo run --example queue-demo --features queue-redis -- serve

APP_QUEUE__DRIVER=redis \
APP_QUEUE__REDIS_NAMESPACE=billing \
APP_QUEUE__REDIS_URL=redis://127.0.0.1:6379/ \
cargo run --example queue-demo --features queue-redis -- queue work demo
```

多个 API/worker 实例必须连接同一 Redis 服务并使用相同 namespace。Redis driver 使用服务端时钟、原子脚本和 token 条件 lease，任务至少执行一次；Redis HA 的持久化和复制策略影响故障切换后的数据保留。该 driver 不支持与数据库事务的原子派发、任务组或数据库事务型 scheduler。

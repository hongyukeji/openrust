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

# OpenRust 性能基准

此 workspace package 集中运行框架性能基准，不属于主框架运行依赖，也不会加入默认构建目标。

## 数据库读写路由

`db_routing` 使用主库与显式只读副本执行串行 `SELECT 1`，预热 20 次后记录吞吐及 p50/p95/p99。它测量连接路由与数据库往返，不代表完整业务吞吐或框架性能结论。

PostgreSQL：

```bash
OPENRUST_TEST_DATABASE_URL=postgres://... \
OPENRUST_TEST_READ_REPLICA_URL=postgres://... \
cargo bench -p performance --bench db_routing --features db-routing-bench,db-postgres
```

MySQL：

```bash
OPENRUST_TEST_DATABASE_URL=mysql://... \
OPENRUST_TEST_READ_REPLICA_URL=mysql://... \
cargo bench -p performance --bench db_routing --features db-routing-bench,db-mysql
```

默认测量 500 个样本；可用 `OPENRUST_DB_ROUTE_BENCH_SAMPLES` 覆盖，最少 100 个。该基准连接真实数据库，不应使用生产数据或生产凭据。

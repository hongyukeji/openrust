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

## HTTP 并发基线

`scripts/http_load.py` 使用 Python 标准库对正在运行的 HTTP 服务发起固定并发请求，按 HTTP 状态码/连接错误分类，并输出每轮吞吐及 p50/p95/p99。脚本不记录完整 URL、查询参数、请求头或正文；不要在命令行参数中放入真实凭据。压测时应使用隔离测试账户、数据和服务。

```bash
python3 tools/performance/scripts/http_load.py \
  --url http://127.0.0.1:3000/api \
  --requests 300000 --concurrency 64 --rounds 3
```

`--header` 可重复指定请求头，`--method` / `--body` 可用于业务请求。结果只描述目标路径、机器、并发和服务配置；客户端与服务同机时，客户端 CPU 可能成为瓶颈。脚本不会自动采集服务进程 CPU/RSS；应按实际 server 可执行文件 PID 定时执行 `ps -o %cpu=,rss=`，并记录采样间隔。默认语义状态会将 JSON API 的 503/429 返回为对应 HTTP 状态；只有显式配置 `[api].http_status = "always_ok"` 时才映射为 HTTP 200。过载验收应使用独立轮次和较小的服务并发上限，核对拒绝状态、对应 metrics、在途资源回落及恢复请求，不把被拒请求算作成功吞吐。

2026-10-27 的开发机专项样本：macOS 27.0.1、arm64、16 核、64 GiB；release 根 `server`、`metrics` feature、只请求 `GET /api` 88 字节 JSON、连接复用、64 并发，ApacheBench 连续三轮共 900,000 请求、零失败，吞吐为 146,633–148,285 req/s，p95/p99 均为 1 ms。压力期间服务 RSS 峰值约 33 MiB，`ps` 采样 CPU 峰值 593%（约 6 核）；结束后在途指标为 0、恢复请求 200。此端点不访问数据库，结果是本机轻量 HTTP 下界，不代表认证、数据库业务、跨机网络或生产性能。

并发上限设为 1 的真实 TCP 过载样本使用 128 个客户端线程发起 5,000 个 `GET /api`：200 为 4,589、503 为 411，p50/p95/p99 为 9.949/20.126/25.974 ms；Prometheus `openrust_http_overloaded_total` 为 411、在途数最终为 0，随后请求返回 200。过载预期拒绝不会逐请求输出 ERROR 日志。该样本用于校验拒绝、指标和恢复语义；同机客户端开销及短时长度不作为性能基线。

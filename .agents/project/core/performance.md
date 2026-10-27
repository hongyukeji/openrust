# performance

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：独立 workspace package，集中维护 OpenRust 框架性能基准；不进入框架运行依赖或默认构建目标。

## 边界

`tools/performance/` 是维护者性能验证工具，不是框架模块、用户业务应用或正确性测试工具。它通过 workspace path dependency 使用根 `openrust` package，并按 feature 转发数据库驱动。增加 benchmark 不能宣称框架性能提升，结论须记录负载、版本、运行环境和指标。

## 当前基准

| 路径 | 行为 |
| --- | --- |
| `tools/performance/benches/db_routing.rs` | 连接 PostgreSQL/MySQL 主库和只读副本，运行串行 `SELECT 1`，输出吞吐及 p50/p95/p99 |

运行方式与测试 URL 见 [performance README](../../../tools/performance/README.md)。根 package 不再声明该 bench target；普通 `cargo test` 不运行基准。完整 HTTP/业务性能、CPU/内存和集群负载验收仍按 [性能计划](../plans/v1-performance-and-readiness.md) 执行。

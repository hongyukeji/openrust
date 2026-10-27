# 独立 consumer 与拓扑验收

这里保存需要独立 Cargo manifest、公开 API 边界或真实服务拓扑的框架验收工程。它们不是面向用户的多个示例应用；用户综合示例统一位于 [`apps/example/`](../../apps/example/README.md)。

| Consumer | 验收范围 |
| --- | --- |
| `cache-custom-driver` | 公开缓存 driver 接入 |
| `data-exchange` | 独立下游数据交换 API |
| `db-custom-sqlite` | SeaORM SQLite 的受限自定义连接边界 |
| `grpc` | gRPC 组合、TLS、真实 TCP 与 Envoy 多节点路由 |
| `query-authorization` | 下游类型化查询授权 API |
| `queue-custom-driver` | 公开队列 driver 扩展 |
| `realtime` | Redis 跨进程 WebSocket/SSE 路由与故障语义 |

这些 package 使用本仓库源码路径，不属于根 workspace 成员；各自按 README 命令独立验证。

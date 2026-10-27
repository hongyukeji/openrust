# WebSocket 与 SSE 实时路由示例

本示例启动两个独立 API 进程，并使用 Redis Pub/Sub 将一个节点发出的消息路由到另一个节点上的 WebSocket 与 SSE 客户端。

## 准备 Redis

复用本机 Redis 8，或自行运行兼容 Redis 协议的服务。不要在已有服务不适用时覆盖其数据目录。本示例不保存消息历史，Redis 只负责在线节点间扇出。

## 运行验收

在仓库根目录执行：

```bash
OPENRUST_REALTIME_TEST_REDIS_URL=redis://127.0.0.1:6379/ \
  cargo test --locked --manifest-path examples/realtime/Cargo.toml --test cluster -- --nocapture
```

该测试会构建并启动两个实际 API 子进程，建立 WebSocket 与 SSE 长连接，在另一节点发送并确认消息，验证拒绝不允许的 Origin、跨节点撤销及 SIGTERM 时关闭 WebSocket/SSE。测试末尾还会启动一个指向不可达 Redis 地址的进程，确认发布返回 `realtime_transport_unavailable`，不会静默降级成本地成功。

## 应用接入边界

- 在应用中启用 `realtime`，安装 `RealtimePlugin`；需要跨节点路由时再启用 `realtime-redis` 并配置 Redis URL。
- 应用自行注册端点、认证请求、检查 Origin 和授权频道。不得从客户端直接信任用户、租户或频道身份。
- `Realtime::connect` 接收授权截止时间；应用应取凭据与业务授权两者中较早的截止时间。Session/PAT 或业务权限变更时，应用显式调用 `Realtime::revoke_user`。
- WebSocket 仅处理在线服务端推送，不接受应用业务输入帧；SSE 不提供 `Last-Event-ID` 离线恢复。
- Redis Pub/Sub 不提供持久化、远端接收确认或 exactly-once。断连窗口内消息可能丢失；需要历史通知时另用持久化业务表与队列。
- 本机队列先于 Redis 发布；发布错误时本机客户端可能已收到消息。调用方重试时应考虑重复投递并使用业务幂等。
- 同一 Redis namespace 内所有节点都能读取 envelope。不要在消息中放入秘密或不应由所有同 namespace 节点读取的数据。
- 每节点连接数、消息大小、每连接队列和总队列字节数均有上限；慢连接会被关闭。跨节点 Redis 故障明确报错，不会改成本地 transport。

可调配置见 RealtimeConfig 与 [ADR-059](../../.agents/project/decisions/adr-059-realtime-transport.md)。完整集群、负载均衡滚动升级与 PostgreSQL/MySQL 总体验收仍由 v1 最终门禁负责。

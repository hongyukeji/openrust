# 实时通信

框架代码位于根模块 openrust::realtime，按 realtime feature 启用；realtime-redis 在其上提供可选 Redis Pub/Sub transport。API 接入由应用路由负责：AuthLayer 与 CurrentUser、Origin 校验、频道授权和 authorized_until 均由应用明确处理。

Realtime::connect 建立有界本地连接；send_to_user 按用户与频道将 JSON 推送到本机连接并发布到配置的共享 transport；revoke_user 关闭本机连接并通知其他节点。公开 RealtimeTransport 可供外部实现替换共享路由。Redis envelope 含 app/namespace topic、节点 ID、用户 ID、频道、消息与过期时刻；同 namespace 节点同属一个消息可见性信任边界。

跨节点发布错误向调用方返回，不本地降级。本机队列先于共享发布处理，因此共享发布失败时，本机连接可能已收到消息而调用仍报错；调用方重试须按可能重复的外部副作用处理。Pub/Sub 只提供在线 best-effort 扇出，无持久回放、远端确认或 exactly-once。入站用户/频道只由可信应用端点解析。SSE 带心跳并关闭连接，不支持 Last-Event-ID；WebSocket 仅处理关闭和协议心跳帧。连接过期、显式撤销及 server shutdown 都会回收会话。

验证边界与公共配置见 [ADR-059](../decisions/adr-059-realtime-transport.md)；真实两 API 进程路由与依赖故障验收见 [memory/testing.md](../memory/testing.md#M5-WebSocket-与-SSE-跨节点路由)，示例用法见 [examples/realtime](../../../examples/realtime/README.md)。当前实现不自动限制应用自行创建的裸连接、任意 transport 或其他网络 client。

# ADR-059：WebSocket 与 SSE 在线实时传输

> 状态：Accepted
> 日期：2026-10-27

## 背景

OpenRust 需要为应用提供在线 WebSocket 与 SSE 能力，并支持连接位于不同 API 节点时的消息路由。进程内事件不跨节点；队列提供持久任务语义，不适合作为在线帧 transport。Redis Pub/Sub 已是项目可选共享依赖，但不会保存离线消息。

## 决策

- `openrust::realtime` 提供连接注册表、在线发送、WebSocket/SSE 会话与公开 `RealtimeTransport` 扩展契约。应用注册端点、应用认证路由并从已认证 `CurrentUser` 派生用户/频道目标；框架不信任客户端提供的租户或目标身份。
- 标准跨节点 transport 使用现有 `redis` 依赖的可选 `realtime-redis` feature。Redis channel 由应用名、显式 namespace 和协议版本隔离；节点收到 envelope 后只向本地匹配连接派发。Redis Pub/Sub 扇出会让同一 namespace 内的节点读取所有 envelope，因此这些节点属于同一信任边界。
- Redis 发布成功仅表示 Redis 接受命令，不表示远端节点或客户端收到。订阅断开期间消息可丢失；无离线回放、持久确认或 exactly-once。跨节点故障必须返回错误，禁止静默本地回退。
- 本机发送队列在共享 transport 发布前处理；若共享发布失败，本机匹配连接可能已经收到消息但 API 调用仍返回错误。调用方重试须将其视作可能重复的外部副作用，并使用自身业务幂等策略。
- Origin 必须由端点显式校验；CORS 不替代 WebSocket/SSE Origin 防护。握手经 `AuthLayer` 后，应用须在频道加入与连接有效期检查授权。内建 JWT access token 仍按既有契约在 exp 前本地有效；Session/PAT 撤销与业务权限变化须通过公开的连接授权检查/显式撤销 API 生效，不能宣称自动即时撤销。
- 每节点连接数、每连接消息数和字节数、入站/出站帧大小、心跳及发布 payload 均有有限上限。慢消费者按配置断开，不静默吞帧。SSE 只服务端推送，使用注释心跳，不支持离线 replay 或 `Last-Event-ID` 恢复。
- `RealtimePlugin` 注册服务并由应用后台任务 supervisor 持有 Redis listener。停机停止新会话，向活动会话发送关闭信号，并在 HTTP server 的统一停机 deadline 内完成任务 join；超时按共享生命周期策略 abort。

## 影响

应用必须安装 `RealtimePlugin`，选择本地 transport 或显式启用 Redis transport，按应用自有路由使用 `CurrentUser`、Origin allowlist 和连接授权检查。跨节点 payload 对同一 Redis namespace 的所有节点可见，不应包含不必要的秘密。通知中心和离线投递仍由应用使用 Queue/持久存储实现。

# 广播

## 广播连接

通过 `BROADCAST_CONNECTION` 选择 `config/broadcasting.rs` 中的连接。默认 `null` 连接丢弃投递。`log` 驱动将频道名、事件名、载荷和来源套接字写入应用日志。

Provider 可以解析 `BroadcastManager`，用 `extend` 注册驱动名和构造函数。管理器将所选连接的配置传给该构造函数。驱动实现 `Broadcaster`，负责投递事件并生成授权响应。

## 频道授权

用 `Broadcast::channel` 注册规则，并通过 `Routing::channels` 或 `withBroadcasting` 加载。框架在 Web 中间件组启用 GET 和 POST `/broadcasting/auth`。该路由排除请求伪造防护中间件，订阅仍需通过驱动的授权检查。

`orders.{id}` 等规则接收已认证用户和命名参数。允许私有订阅时返回 `ChannelAuthorization::Allowed`，公开在线成员资料时返回 `ChannelAuthorization::Presence(data)`。用 `channelWithOptions` 指定守卫的查找顺序。运行 `artisan channel:list` 查看注册的规则。

## 定义广播事件

实现 `ShouldBroadcast`，在 Provider 中调用 `Dispatcher::registerBroadcast::<E>()` 注册编译后的类型。常规事件分发随后将投递送入队列。实现 `ShouldBroadcastNow` 的事件使用 `registerBroadcastNow::<E>()` 注册，分发时同步投递。

`broadcastOn` 选择公共、私有或在线频道。`broadcastAs` 和 `broadcastWith` 自定义名称和载荷。默认使用事件名和 `broadcastData` 显式返回的字段。`broadcastWhen` 返回 false 时跳过投递。队列保存序列化后的投递数据。

实现 `ShouldDispatchAfterCommit` 并通过 `Dispatcher::markAfterCommit::<E>()` 标记事件后，活跃事务中的事件将延迟到提交后分发。队列广播也遵循 `broadcastAfterCommit` 和队列连接的 `after_commit`。使用 `EventContext::toOthers(&request)` 将请求的 `X-Socket-ID` 传给驱动，供驱动排除来源连接。

内置连接为 `null` 和 `log`。启用 [Reverb 扩展包](../packages/reverb.md) 后可使用兼容 Pusher 的传输。

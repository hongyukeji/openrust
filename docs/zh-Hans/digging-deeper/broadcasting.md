# 广播

## 广播连接

通过 `BROADCAST_CONNECTION` 选择 `config/broadcasting.rs` 中的连接。默认 `null` 连接丢弃投递。`log` 驱动将频道名、事件名、载荷和来源套接字写入应用日志。

Provider 可以解析 `BroadcastManager`，用 `extend` 注册驱动名和构造函数。管理器将所选连接的配置传给该构造函数。驱动实现 `Broadcaster`，负责投递事件并生成授权响应。

## 频道授权

用 `Broadcast::channel` 注册规则，并通过 `Routing::channels` 或 `with_broadcasting` 加载。框架在 Web 中间件组启用 GET 和 POST `/broadcasting/auth`。该路由排除请求伪造防护中间件，订阅仍需通过驱动的授权检查。

`orders.{id}` 等规则接收已认证用户和命名参数。允许私有订阅时返回 `ChannelAuthorization::Allowed`，公开在线成员资料时返回 `ChannelAuthorization::Presence(data)`。用 `channel_with_options` 指定守卫的查找顺序。运行 `artisan channel:list` 查看注册的规则。

对于 `orders.{order}`，使用 `channel_with_bindings` 和 `ChannelBindings::default().model::<Order>("order")` 声明模型参数。实现 `ChannelHandler::join_bound`，通过 `models.get::<Order>("order")` 读取模型。查询使用模型的路由键，支持 slug 等自定义键。隐式模型缺失时返回 HTTP 403。同名的 `Router::bind` 或 `Router::model` 显式绑定优先，查找不到时返回 HTTP 404。各频道模型分别解析。

重叠规则按注册顺序执行，使用首个匹配规则指定的守卫。返回 `ChannelAuthorization::Abstain` 继续尝试下一条规则，返回 `Denied` 则立即拒绝。

## 定义广播事件

实现 `ShouldBroadcast`，在 Provider 中调用 `Dispatcher::register_broadcast::<E>()` 注册编译后的类型。常规事件分发随后将投递送入队列。实现 `ShouldBroadcastNow` 的事件使用 `register_broadcast_now::<E>()` 注册，分发时同步投递。

`broadcast_on` 选择公共、私有或在线频道。`broadcast_as` 和 `broadcast_with` 自定义名称和载荷。默认使用事件名和 `broadcast_data` 显式返回的字段。`broadcast_when` 返回 false 时跳过投递。队列保存序列化后的投递数据。

实现 `ShouldDispatchAfterCommit` 并通过 `Dispatcher::mark_after_commit::<E>()` 标记事件后，活跃事务中的事件将延迟到提交后分发。队列广播也遵循 `broadcast_after_commit` 和队列连接的 `after_commit`。使用 `EventContext::to_others(&request)` 将请求的 `X-Socket-ID` 传给驱动，供驱动排除来源连接。

## 发送匿名广播 {#anonymous-broadcasting}

使用 `Broadcast::on("news")`、`Broadcast::private("orders.42")` 或 `Broadcast::presence("rooms.42")` 创建匿名广播。`Broadcast::on` 也接受频道列表。通过 `r#as` 设置发送名称，通过 `with` 设置 JSON 载荷，通过 `via` 选择广播连接。默认发送名称是 `AnonymousEvent`。

等待 `send()` 执行常规队列投递，或等待 `send_now()` 同步发送。请求包含 `X-Socket-ID` 时，`to_others(&request)` 排除来源套接字。默认受众包含来源套接字。

## 唯一队列广播 {#unique-broadcasts}

实现 `contracts::broadcasting::ShouldBeUnique` 后，用 `Dispatcher::register_unique_broadcast::<E>()` 注册。唯一性由事件的 Rust 类型和 `unique_id()` 共同确定。用 `unique_for()` 设定锁租期秒数，用 `unique_via()` 选择具名缓存存储。默认 ID 为空，锁无到期时间并使用默认缓存存储。

锁已被持有时，队列派发 `UniqueJobSkipped`。重试期间继续持锁，任务成功或永久失败后释放。同步广播无需唯一锁。显式 Rust 事务中的唯一队列投递在提交时获取锁，事务回滚会丢弃待投递任务。事务活跃期间，唯一投递若显式指定 `broadcast_after_commit() == Some(false)`，会返回配置错误。

内置连接为 `null` 和 `log`。启用 [Reverb 扩展包](../packages/reverb.md) 后可使用兼容 Pusher 的传输。
## 用户认证路由

`Broadcast::user_routes(group, connection)` 注册 GET、POST `/broadcasting/user-auth`，使用 web 中间件组并豁免 CSRF。默认路由组或连接传入 `None`。通过 `authenticateUserUsing` 登记可读取当前请求的解析回调，返回用户 JSON 数据，访客返回无用户。用户认证和频道认证各自独立。

Reverb 驱动校验 socket ID，并按 Pusher 协议签名用户认证响应，具名连接使用各自凭据。其他未实现的驱动返回 `UnsupportedUserAuthentication`。该接口不会为 Reverb 服务端增加 `pusher:signin` 处理器；托管平台和完整客户端登录仍需单独集成。

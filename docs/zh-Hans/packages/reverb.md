# Reverb

## 安装

在应用 `Cargo.toml` 的 `[features].default` 列表中加入 `reverb`，保留已有功能。该功能注册 Reverb Provider、`reverb` 广播驱动和 Artisan 命令。源码命令使用 `./artisan`。

包自身提供默认服务端配置，启用 feature 不会生成 `config/reverb.rs`。需要应用覆盖时运行 `./artisan vendor:publish --tag=reverb-config`，在 `config/mod.rs` 的模块声明和配置映射中分别加入 `mod reverb;` 与 `("reverb".to_owned(), reverb::load(env)?),`，然后重新构建。显式执行 `init --extensions=reverb` 会发布并登记此配置；重复初始化保留已有定制，除非传入 `--force`。

使用 `REVERB_APP_ID`、`REVERB_APP_KEY` 和 `REVERB_APP_SECRET` 设置监听端与发布端共用的凭据。客户端使用公开密钥和 WebSocket 地址，密钥保留在服务端。

## 运行服务

设置 `REVERB_ENABLED=true` 使用默认的内嵌模式。启动应用时会绑定 HTTP 和 Reverb 两个监听器。`REVERB_SERVER_HOST`、`REVERB_SERVER_PORT` 和 `REVERB_SERVER_PATH` 配置 Reverb 监听器。`REVERB_HOST`、`REVERB_PORT` 和 `REVERB_SCHEME` 配置广播驱动的发布目标。本地监听器未配置 TLS 代理时，使用 `REVERB_SCHEME=http`。

需要独立进程时，设置 `REVERB_MODE=standalone` 并通过 Artisan 启动：

```sh
./artisan reverb:start --host=127.0.0.1 --port=8080
./artisan reverb:restart
```

`reverb:start --hostname=edge.example.com` 在绑定地址旁显示客户端访问的主机名，默认读取 `REVERB_HOST`。实际绑定仍由 `--host` 和 `--port` 决定。使用 `--debug` 在终端显示连接、协议事件和发布诊断。

重启命令向应用缓存写入信号。同一台主机上可使用 `CACHE_STORE=file` 共享信号。独立进程收到信号后关闭监听器和连接，可由进程管理器重新启动。内嵌进程随应用关停和重启。

## 广播与扩展 {#broadcasting-and-scaling}

设置 `BROADCAST_CONNECTION=reverb`，按[广播文档](../digging-deeper/broadcasting.md)启用私有和在线频道授权。Pusher 协议客户端连接配置前缀下的 `/app/{key}`。生产环境可在应用的 `allowed_origins` 设置受信任的主机名，由反向代理终止 TLS。

`reverb` 和 `pusher` 连接通过 HTTPS 发布时，默认校验证书及主机名。使用私有 CA 时，在 `config/broadcasting.rs` 对应连接的 `client_options.verify` 中设置 PEM CA 证书包的绝对路径：

```rust
serde_json::json!({
    "client_options": { "verify": "/etc/openrust/reverb-ca.pem" }
})
```

该证书包替换此连接的默认信任根，服务端证书链和主机名仍须匹配。证书包不可读、无效或为空时，解析连接立即失败。省略 `verify` 或设为 `true` 使用平台信任；`false` 关闭证书校验，仅适合隔离的本地调试。此配置尚未提供其他 Guzzle 客户端选项。

在各实例设置 `REVERB_SCALING_ENABLED=true`。同一应用的节点共用 `REVERB_APP_ID`、`REVERB_APP_KEY`、`REVERB_APP_SECRET`、`REVERB_SCALING_CHANNEL` 和 Redis 连接配置。可以使用 `REDIS_URL` 或分别设置 Redis 参数。不同应用共用扩展频道时应使用不同的应用 ID。同一个扩展频道的节点应运行相同版本的 OpenRust Reverb。

订阅在线频道时，客户端收到全局成员列表和人数。同一用户建立多条连接仍只计一人，其他订阅者只会看到该用户的首次加入和最后一次离开。Redis 可用时，节点正常退出会移除其连接。节点异常退出后，仍有订阅者的节点大约每 2 秒检查一次，清理 10 秒租期已到期的连接。新订阅返回成员列表前也会清理过期连接。

Redis Pub/Sub 跨节点投递签名的 HTTP 发布和已授权的客户端事件。客户端事件只发给同一应用、同一频道的其他订阅连接，排除发送者的连接。节点失去 Redis 订阅时会关闭本节点的在线频道连接，由客户端在恢复后重连并重新订阅。事件尽力投递，断线期间错过的事件不会补发。

## Pusher HTTP API 与缓存频道 {#pusher-http-api}

向 `POST /apps/{id}/events` 或 `POST /apps/{id}/batch_events` 发送签名请求。批量请求的 `batch` 数组按顺序投递，每项包含事件名、频道、JSON 编码的 `data`，以及可选的 `socket_id` 和 `info`。服务端先校验整个批次，再投递事件。两个接口都可返回请求的频道统计。

使用签名 GET 请求查询 `/apps/{id}/connections`、`/apps/{id}/channels`、`/apps/{id}/channels/{channel}` 或 `/apps/{id}/channels/{channel}/users`。频道列表可用 `filter_by_prefix` 筛选，使用 `info` 选择 `subscription_count`、`user_count` 或 `cache` 等字段。启用 Redis 扩展后，监听器合并各响应节点的统计，并对在线频道用户 ID 去重。

带有 `cache-`、`private-cache-` 或 `presence-cache-` 前缀的频道，在订阅成功后重放最近一次事件。缓存为空时发送 `pusher:cache_miss`。频道授权仍然适用。遵循 Reverb 的缓存生命周期，各监听器在有本机订阅者时保留缓存，最后一位本机订阅者离开后清除。

## Echo 客户端 {#echo-client}

安装 `laravel-echo` 和 `pusher-js`。以下示例连接本地未启用 TLS 的 Reverb 监听器。配置路径前缀时，将 `wsPath` 设为服务端路径。

```js
import Echo from "laravel-echo"
import Pusher from "pusher-js"

const echo = new Echo({
  broadcaster: "reverb",
  Pusher,
  key: "your-public-app-key",
  wsHost: window.location.hostname,
  wsPort: 8080,
  forceTLS: false,
  enabledTransports: ["ws"],
})

echo.channel("news").listen(".article.updated", event => console.log(event))
```

使用 `echo.private("orders.42")` 订阅私有频道，使用 `echo.join("rooms.42")` 加入在线频道。两者都通过应用的 `/broadcasting/auth` 和已注册的频道守卫授权。连接恢复后，Echo 自动重新订阅。

## Pusher Channels {#pusher-channels}

`reverb` Cargo 特性也注册 `pusher` 广播驱动。设置 `BROADCAST_CONNECTION=pusher`、`PUSHER_APP_ID`、`PUSHER_APP_KEY`、`PUSHER_APP_SECRET` 和 `PUSHER_APP_CLUSTER`。默认发布端点为 `https://api-{cluster}.pusher.com:443`。连接兼容 Pusher 的测试端点时，可覆盖 `PUSHER_HOST`、`PUSHER_PORT` 和 `PUSHER_SCHEME`。

使用托管 Pusher Channels 时，将 Echo 的 `broadcaster` 设为 `"pusher"`，并传入公开 Key 与 Cluster。私有和在线频道仍通过应用的 `/broadcasting/auth` 和频道规则授权。

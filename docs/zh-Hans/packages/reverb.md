# Reverb

## 安装

构建时启用 Cargo 的 `reverb` 功能。该功能注册 Reverb Provider、`reverb` 广播驱动和 Artisan 命令。

```sh
cargo build --features reverb
```

使用 `REVERB_APP_ID`、`REVERB_APP_KEY` 和 `REVERB_APP_SECRET` 设置监听端与发布端共用的凭据。客户端使用公开密钥和 WebSocket 地址，密钥保留在服务端。

## 运行服务

设置 `REVERB_ENABLED=true` 使用默认的内嵌模式。启动应用时会绑定 HTTP 和 Reverb 两个监听器。`REVERB_SERVER_HOST`、`REVERB_SERVER_PORT` 和 `REVERB_SERVER_PATH` 配置 Reverb 监听器。`REVERB_HOST`、`REVERB_PORT` 和 `REVERB_SCHEME` 配置广播驱动的发布目标。本地监听器未配置 TLS 代理时，使用 `REVERB_SCHEME=http`。

需要独立进程时，设置 `REVERB_MODE=standalone` 并通过 Artisan 启动：

```sh
./target/debug/main artisan reverb:start --host=127.0.0.1 --port=8080
./target/debug/main artisan reverb:restart
```

重启命令向应用缓存写入信号。同一台主机上可使用 `CACHE_STORE=file` 共享信号。独立进程收到信号后关闭监听器和连接，可由进程管理器重新启动。内嵌进程随应用关停和重启。

## 广播与扩展

设置 `BROADCAST_CONNECTION=reverb`，按[广播文档](../digging-deeper/broadcasting.md)启用私有和在线频道授权。Pusher 协议客户端连接配置前缀下的 `/app/{key}`。生产环境可在应用的 `allowed_origins` 设置受信任的主机名，由反向代理终止 TLS。

在各实例设置 `REVERB_SCALING_ENABLED=true`，使用 Redis 跨进程投递签名的 HTTP 发布事件。各实例共用 `REVERB_SCALING_CHANNEL` 和 Redis 设置。可以使用 `REDIS_URL`，或分别配置 Redis 主机、凭据、端口和数据库。频道成员状态和客户端发起的事件由各自的监听器处理。

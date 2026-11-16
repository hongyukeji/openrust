# 08-02 Reverb 与客户端协议

对照固定 [Laravel Reverb](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/reverb.md)、[Pusher Server](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/Server.php)、[EventsController](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/Http/Controllers/EventsController.php) 和 [RestartServer](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Servers/Reverb/Console/Commands/RestartServer.php)。Goravel 固定版参考应用编译型装配。旧版 `6d1a8192` 无可复用的 Reverb、Pusher 或 WebSocket 源码及测试。扩展包接入现有应用 Provider、频道守卫、广播连接、Artisan 与缓存。

- Cargo 特性 `reverb` 启用可选扩展。内嵌 HTTP 和 Reverb 在 Tokio 中先绑定再就绪，共用关停信号。独立 `reverb:start` 支持监听主机、端口和路径覆盖，`reverb:restart` 通过共享缓存让独立进程退出，交由进程管理器重新拉起。
- Pusher 协议处理 WebSocket 握手、Origin、连接限制、心跳、公开／私有／在线频道、成员变更和客户端事件。HMAC 验证频道授权和 HTTP 发布。框架通过频道指定的守卫解析在线成员身份，保留字符串身份中的前导零。
- Redis Pub/Sub 跨进程投递已签名的 HTTP 发布及客户端事件，按应用和频道分发并排除来源套接字。启用扩展时在线频道返回全局成员列表和人数。Rust 通过有界队列限制慢客户端积压，并关闭无法跟上的连接。
- TLS 在反向代理终止。Pusher 缓存频道、统计和批量事件接口、Artisan 的 `--hostname` 与 `--debug` 仍待补齐。Echo 浏览器库及外部托管 Pusher 服务未验收。

## 验收

- Rust 1.94.0、macOS Apple Silicon。锁定离线工作区启用 Reverb 后 529 项通过，4 个忽略项由集成测试作为子进程启动。格式和严格 Clippy 通过。
- Tokio Tungstenite 与 Python websockets 17.3 通过真实监听器验收。覆盖 Origin 拒绝、连接上限、私有签名拒绝、在线成员增删、签名正文篡改、跨应用隔离、重连、关停、端口释放，以及框架实际 HTTP 授权、字符串身份与广播发布。
- `python3 tests/reverb_runtime.py` 以实际应用二进制验收内嵌／独立模式、绑定冲突、Artisan 隔离及重启。设置 `REVERB_TEST_REDIS_URL` 后，使用真实 Redis 8.10.3 和两个独立应用进程验证跨进程单次投递与来源排除。应用迁移、`inspire`、首页和 `/up` 冒烟通过。
- 三语 VitePress 构建、12 项通用浏览器检查及 6 项从广播页到 Reverb 的导航检查通过。

下一单元 08-03 gRPC 扩展。原工作区 gRPC 监听与锁文件改动单独保留。

## 2026-11-16 Redis 集群验收

- 对照 Reverb v1.8.1 的 [ClientEvent](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/ClientEvent.php)、[EventDispatcher](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/EventDispatcher.php) 与 [PresenceChannels](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/Channels/Concerns/InteractsWithPresenceChannels.php)。保留客户端事件的授权、来源排除和已认证成员 ID。固定版从进程内连接管理器读取订阅成员并用 Redis 分发成员事件。OpenRust 为满足全局快照，在同一 Redis Lua 操作中维护连接租约、按用户去重并投递快照及增删通知，保持同频道订阅事件的顺序。配置和连接保活继续遵循现有 Reverb 方式。
- 上游 [RedisPublishClient](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Servers/Reverb/Publishing/RedisPublishClient.php) 可暂存在断线期间产生的发布。OpenRust 通过 Pub/Sub 尽力投递，失败时 HTTP 发布返回 503、客户端事件返回协议错误。Redis 订阅断开会关闭在线频道连接并重新订阅，由客户端重连建立新成员状态。Redis 保留的旧条目若未能及时清理，按 10 秒连接租期失效，仍有订阅者的节点约每 2 秒清扫，新订阅也会移除过期条目。建议部署使用同版本 OpenRust 节点，Redis 消息包含 OpenRust 内部帧。
- Rust 1.94.0，macOS Apple Silicon。隔离 Git 基线仅覆盖本批 Reverb 源码。离线锁定 Reverb 测试 7 项、格式、全目标严格 Clippy、实际应用二进制 `cargo build --features reverb --bin main`、三语 VitePress 构建及 Chromium 12 项通用和 6 项扩展锚点检查通过。
- `python3 tests/reverb_runtime.py` 使用同一实际二进制启动三个内嵌 HTTP／Reverb 进程。两个节点共享应用凭据，第三节点使用独立应用 ID 与 Key，三个节点使用独立测试 MySQL 8.4.13、Redis 8.10.3 和同一扩展频道。节点一执行 `artisan migrate --force`，节点二读取 `migrate:status`，三个节点分别访问 `/up`。先回归独立／内嵌启动、端口冲突、Artisan 隔离、应用重启和签名 HTTP 发布跨进程单次投递、来源排除。
- 真实 WebSocket 验收同一用户跨节点多连接只计一人、成员原始 JSON、跨节点首次加入及最后离开、断线重连、节点 SIGKILL 后租期清除、节点 SIGTERM 后成员移除。私有和在线客户端事件跨节点单次到达，来源连接、其他频道和其他应用均未收到，非法签名和未加入频道的发布被拒绝，伪造成员 ID 被忽略。重启带持久化的 Redis 容器后，在线频道连接关闭，节点重新订阅，客户端重连获得无残留成员的快照。用固定回环发布端口复现重启，测试脚本可通过 `REVERB_TEST_BINARY`、`REVERB_TEST_MYSQL_URL`、`REVERB_TEST_REDIS_URL` 和可选的 `REVERB_TEST_RESTART_REDIS_CONTAINER` 复验。

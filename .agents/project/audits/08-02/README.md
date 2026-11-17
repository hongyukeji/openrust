# 08-02 Reverb 与客户端协议

对照固定 [Laravel Reverb](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/reverb.md)、[Pusher Server](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/Server.php)、[EventsController](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/Http/Controllers/EventsController.php) 和 [RestartServer](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Servers/Reverb/Console/Commands/RestartServer.php)。Goravel 固定版参考应用编译型装配。旧版 `6d1a8192` 无可复用的 Reverb、Pusher 或 WebSocket 源码及测试。扩展包接入现有应用 Provider、频道守卫、广播连接、Artisan 与缓存。

- Cargo 特性 `reverb` 启用可选扩展。内嵌 HTTP 和 Reverb 在 Tokio 中先绑定再就绪，共用关停信号。独立 `reverb:start` 支持监听主机、端口和路径覆盖，`reverb:restart` 通过共享缓存让独立进程退出，交由进程管理器重新拉起。
- Pusher 协议处理 WebSocket 握手、Origin、连接限制、心跳、公开／私有／在线频道、成员变更和客户端事件。HMAC 验证频道授权和 HTTP 发布。框架通过频道指定的守卫解析在线成员身份，保留字符串身份中的前导零。
- Redis Pub/Sub 跨进程投递已签名的 HTTP 发布及客户端事件，按应用和频道分发并排除来源套接字。启用扩展时在线频道返回全局成员列表和人数。Rust 通过有界队列限制慢客户端积压，并关闭无法跟上的连接。
- TLS 在反向代理终止。缓存频道、统计、批量事件、Artisan 参数及 Echo 的后续验收见下文。

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

## 缓存频道、统计与批量事件

- 固定 Reverb 在订阅成功后重放最近的业务帧，缓存为空时只发送 `pusher:cache_miss` 的事件与频道。内部成员通知不写入缓存。最后一个本机订阅离开时清除缓存。托管 Pusher 的长时间保留行为另有契约。 [固定 CacheChannel](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/Channels/CacheChannel.php) [固定 EventHandler](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/EventHandler.php)
- 在现有 HMAC 校验下接通连接数、频道列表／前缀筛选、单频道占用及成员列表。单次发布与批量发布可返回频道统计；先校验整批，再按顺序投递，并保留各事件的来源排除。Redis 扩展收集响应节点的统计并对在线成员 ID 去重。 [固定 MetricsHandler](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/MetricsHandler.php) [固定 EventsBatchController](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/Http/Controllers/EventsBatchController.php)
- Rust 1.94.0，Reverb 8 项回归、全目标严格 Clippy、含 Reverb 特性的实际二进制构建通过。实际 `reverb:start` 启动两个独立进程和专用 Redis 8.10.3，签名 HTTP 与真实 WebSocket 验证了跨节点连接数、成员去重、批次顺序及来源排除、缓存未命中与重放。内嵌启动、Artisan 隔离、路径覆盖和 `reverb:restart` 回归通过。
- 三语 VitePress 构建以及 12 项浏览器检查和三语新增锚点通过。专用 Redis、临时站点和本批可执行文件已清理。

## Artisan 主机名、诊断与 Echo

- 固定 Reverb 的 `--hostname` 覆盖公开主机名，监听地址仍由主机和端口选项决定。Rust 启动横幅显示实际地址、路径和公开主机名，默认读取 `REVERB_HOST`。 `--debug` 输出连接、事件、频道和发布诊断，日志仅记录元数据。 [固定 StartServer](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Servers/Reverb/Console/Commands/StartServer.php)
- 实际应用二进制验证 `help reverb:start`、独立启动的主机名与路径覆盖、默认主机名、诊断开关、HTTP 健康和签名发布、WebSocket Ping/Pong、 `reverb:restart` 与连接关闭。
- Google Chrome 157 中使用 Laravel Echo 2.2.6 与 pusher-js 8.6.0，实测公开、私有和在线频道订阅、实际框架 HTTP 授权、在线成员数据和签名 HTTP 投递。主动断开底层 WebSocket 后，Echo 自动使用新套接字重新授权和订阅，三个频道各收到一次新事件。可在临时目录安装固定版本客户端，设置 `OPENRUST_REVERB_ECHO_MODULES` 指向其 `node_modules` 后运行 Reverb `transport` 测试。 [固定 Laravel Echo 客户端配置](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/broadcasting.md#client-reverb)
- 8 项 Reverb 回归、工作区全目标全特性严格 Clippy、改动文件格式、三语 VitePress 构建、12 项浏览器检查及三语新增锚点通过。

- 独立 MySQL 8.4.13、Redis 8.10.3 与三个实际应用通过全局在线成员、跨进程客户端事件、应用隔离、节点退出和租期清除。Redis 容器使用固定回环发布端口，实测重启后客户端可重新订阅且成员快照无残留。

外部托管 Pusher 当时因缺少可用凭据维持待验收。

## 08-02 模拟验收与闭合

- 复查本地验收环境，四项托管配置未齐备。按确认的模拟验收口径，使用隔离的本地 Reverb 监听器充当 Pusher 协议服务，使用测试凭据和真实回环 TCP。固定 Laravel 的 Reverb 与 Pusher 共享传输工厂。Rust 在 Reverb Provider 中为 `pusher` 注册同一签名传输，并通过现有 Pusher 连接配置独立解析凭据和端点。 [固定 Laravel BroadcastManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Broadcasting/BroadcastManager.php#L321) [Pusher HTTP API](https://pusher.com/docs/channels/library_auth_reference/rest-api/)
- 真实框架 HTTP 验证拒绝无效身份、私有及在线签名、字符串成员 ID。实际 Artisan 命令通过具名驱动向模拟服务发送签名 HTTP 请求。Chrome 中使用 Laravel Echo 2.2.6 和 pusher-js 8.6.0 的 `pusher` 配置验证公开、私有及在线频道的订阅和送达，断线后取得新套接字并重新授权。重连前后各由 Artisan 发布一次，三个频道分别各收一条。原 `reverb` 驱动的同类浏览器验收通过。
- Rust 1.94.0 锁定离线通过 9 项 Reverb 回归、全工作区严格 Clippy、实际应用二进制构建及内嵌与独立 Artisan 运行回归。三语文档站构建、12 项浏览器检查与三语章节锚点通过。已清理临时客户端、站点和本批测试可执行文件。
- 08-02 按模拟验收口径闭合。托管 Pusher 的 TLS 和部署配置仍待实机验证。

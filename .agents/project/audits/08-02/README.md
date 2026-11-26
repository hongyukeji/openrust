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

## 客户端用户登录与定向投递 {#user-sign-in}

Laravel Reverb 固定版 v1.8.1 的 EventHandler 不处理用户登录。OpenRust 基于已有 `Broadcast::user_routes`、解析回调及 HMAC 构建规则，按 Pusher Channels 7 接通 `pusher:signin`。验证当前应用 Key、当前套接字、原始用户 JSON 及非空字符串 ID。成功响应原样返回已签 JSON。相同 ID 可重复登录，同一连接切换 ID、复用旧套接字的签名及跨应用签名返回协议错误。 [固定 Reverb EventHandler](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/EventHandler.php) · [Pusher 协议](https://pusher.com/docs/channels/library_auth_reference/pusher-websockets-protocol/) · [签名规则及示例](https://pusher.com/docs/channels/library_auth_reference/auth-signatures/)

pusher-js 8.6.0 登录后订阅 `#server-to-user-<id>` 并在重连时重新认证。该频道由每应用连接身份限定，拒绝访客、其他身份订阅和普通客户端事件。签名 HTTP 发布沿用原 REST 验证和本机／Redis 投递，同一用户在不同应用仍隔离。频道统计和调试输出省略用户 ID，调试只保留事件及脱敏频道名。 [固定客户端 UserFacade](https://raw.githubusercontent.com/pusher/pusher-js/v8.6.0/src/core/user.ts) · [固定服务端 SDK 的 sendToUser](https://github.com/pusher/pusher-http-node/blob/v5.2.0/lib/pusher.js)

基线 `d31032e98`，Rust 1.94.0。Reverb 包 12 项通过。真实 WebSocket 覆盖同一身份重复登录、签名改写与重放、无效 JSON 与 ID 类型、身份切换、跨应用阻断、客户端伪造与签名定向投递。Chrome 通过 Echo 2.2.6 和 pusher-js 8.6.0，用真实框架 HTTP 获取签名并重新登录，分别以 `reverb` 和具名 `pusher` 配置验收 HTTP／WS 及 HTTPS／WSS。Artisan 两次分别向登录和重连后的用户送达。HTTPS 替身拒绝默认未信任 CA、错误 CA 及主机名错配。真实二进制 `artisan reverb:start` 检查调试脱敏；同主机两个实际进程、专用 Redis 8.10.3，实测跨节点定向投递、未登录连接阻断、在另一节点重连及重新认证。修改范围格式、严格 Clippy 和三语站点构建与浏览器导航通过。

本轮验收范围是用户登录、相应身份隔离、定向发送和重连。托管 Pusher 实例、跨物理主机、Pusher Watchlist、基于用户 ID 的远端断连管理以及在线频道用户资料继承分别保留原支持范围。一次磁盘停线由项目守卫处理，清理后冷构建。TLS 首轮夹具提前解析了账户而缓存默认传输，在配置 TLS 后再注册解析回调并复验。

<a id="nginx-tls-acceptance"></a>

## Nginx HTTPS 与 WSS 复验

在 `f2c05a42e` 的干净归档中叠加验收入口，Rust 1.94.0。使用本机 Docker Desktop 的 Nginx 1.30.7 镜像 `sha256:caf4e1e032a20f9c3c5e28fc7f25ffd768ad8677b4975c54193075997b3a97e3`。沿用仓库 Nginx 模板的 Host 和转发头、HTTP/1.1 Upgrade 及关闭缓冲设置，临时增加测试证书和 `/socket/` 分流。授权与用户认证进入应用 HTTP 监听器，WebSocket 与签名发布进入 Reverb 监听器。试跑使用默认桥接网络时发布超时，切换为与 Compose 同类型的独立网络后复验通过。 [Nginx WebSocket 转发](https://nginx.org/en/docs/http/websocket.html) · [固定 Reverb 部署指南](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/reverb.md#web-server)

Chrome 使用 Echo 2.2.6 和 pusher-js 8.6.0，通过实际代理验证 `reverb` 和具名 `pusher` 的公开、私有和在线频道，用户认证在自动重连后重新执行，重连前后的 Artisan 定向发布均送达。Rust 发布额外验证信任测试 CA 后送达、未信任 CA／错误 CA／主机名错配时拒绝。普通 HTTP、原 TLS 转发器和 Nginx 三种入口各通过 6 项。独立应用二进制和两项 `artisan reverb:start` 进程复跑 Redis 扩展、签名投递、登录和跨节点重连。

复验入口为 `tests/reverb_tls_acceptance.py --nginx IMAGE -- <transport 测试二进制>`，使用本机 Docker Desktop 镜像和既有 Echo 夹具参数。包装器清理本批证书、容器和独立网络。格式、包全目标严格 Clippy、三语站点构建和浏览器检查通过。证据位于 `target/tmp/reverb-nginx/`。正式域名证书、选定发布入口及物理跨节点部署仍保留验收。

## 定向投递的单节点 Redis 分区 {#redis-user-partition}

使用 `577daf16f` 快照、Rust 1.94.0 和 macOS ARM64 的主应用二进制。两个实际 `artisan reverb:start` 进程使用同一个 Redis 8.10.3。复用 Webhook 验收的 TCP 断连夹具，只中断第一个节点的现有及新建 Redis 连接。固定版 Reverb 的 Redis 发布客户端暂存断线发布，OpenRust 沿用既有 HTTP 503 和尽力投递契约。[固定 Reverb Redis 发布客户端](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Servers/Reverb/Publishing/RedisPublishClient.php) · [Redis Pub/Sub 投递语义](https://redis.io/docs/latest/develop/pubsub/)

用户 `007` 的两条签名登录连接分别驻留在两个节点，用户 `009` 作为隔离参照。断连前，两条目标连接各收到一条事件。分区时，从隔离节点发布返回 HTTP 503。健康节点的发布仍到达其目标连接，同一应用的其他用户及隔离节点均未收到。恢复后，全局连接统计稳定为三条，新事件跨节点各送达一次，分区期间的事件未补发。重连的新套接字拒绝原签名，重新认证后恢复定向投递。

同一二进制复跑内嵌 HTTP、独立 Artisan、签名发布和三应用的全局在线成员、应用隔离、客户端事件、退出清理及 Redis 重启。首轮重启用例因 Docker 重分配随机宿主端口导致客户端仍访问旧端口。另用独立实例确认端口会变，固定回环映射后复验通过。MySQL 8.4.13 与 Redis 均为独立测试实例。日志保存在 `target/tmp/reverb-user-redis-partition/`。正式域名证书、选定代理和跨物理节点部署仍按目标环境验收。

工作区格式检查、主应用与 Reverb 扩展全目标严格 Clippy、三项签名及用户认证契约测试通过。三语站点构建及根路径与 `/docs/` 下的 Reverb 页面浏览器检查通过。验收容器、隔离源码与文档产物已清理。

# 08-02 Reverb 与客户端协议

对照固定 [Laravel Reverb](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/reverb.md)、[Pusher Server](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/Server.php)、[EventsController](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Protocols/Pusher/Http/Controllers/EventsController.php) 和 [RestartServer](https://github.com/laravel/reverb/blob/70e3d28ed31466da34de0c055f3681b75a5a538c/src/Servers/Reverb/Console/Commands/RestartServer.php)。Goravel 固定版参考应用编译型装配。旧版 `6d1a8192` 无可复用的 Reverb、Pusher 或 WebSocket 源码及测试。扩展包接入现有应用 Provider、频道守卫、广播连接、Artisan 与缓存。

- Cargo 特性 `reverb` 启用可选扩展。内嵌 HTTP 和 Reverb 在 Tokio 中先绑定再就绪，共用关停信号。独立 `reverb:start` 支持监听主机、端口和路径覆盖，`reverb:restart` 通过共享缓存让独立进程退出，交由进程管理器重新拉起。
- Pusher 协议处理 WebSocket 握手、Origin、连接限制、心跳、公开／私有／在线频道、成员变更和客户端事件。HMAC 验证频道授权和 HTTP 发布。框架通过频道指定的守卫解析在线成员身份，保留字符串身份中的前导零。
- Redis Pub/Sub 跨进程投递已签名的 HTTP 发布，按应用和频道分发，并排除请求指定的来源套接字。订阅断开后尝试重新订阅。频道成员和客户端事件仍在单个监听器内处理。Rust 通过有界队列限制慢客户端积压，并关闭无法跟上的连接。
- TLS 在反向代理终止。Redis 全局在线成员、跨节点客户端事件、Pusher 缓存频道及统计和批量事件接口、Artisan 的 `--hostname` 与 `--debug` 仍待补齐。Echo 浏览器库及外部托管 Pusher 服务未验收。

## 验收

- Rust 1.94.0、macOS Apple Silicon。锁定离线工作区启用 Reverb 后 529 项通过，4 个忽略项由集成测试作为子进程启动。格式和严格 Clippy 通过。
- Tokio Tungstenite 与 Python websockets 17.3 通过真实监听器验收。覆盖 Origin 拒绝、连接上限、私有签名拒绝、在线成员增删、签名正文篡改、跨应用隔离、重连、关停、端口释放，以及框架实际 HTTP 授权、字符串身份与广播发布。
- `python3 tests/reverb_runtime.py` 以实际应用二进制验收内嵌／独立模式、绑定冲突、Artisan 隔离及重启。设置 `REVERB_TEST_REDIS_URL` 后，使用真实 Redis 8.10.3 和两个独立应用进程验证跨进程单次投递与来源排除。应用迁移、`inspire`、首页和 `/up` 冒烟通过。
- 三语 VitePress 构建、12 项通用浏览器检查及 6 项从广播页到 Reverb 的导航检查通过。

下一单元 08-03 gRPC 扩展。原工作区 gRPC 监听与锁文件改动单独保留。

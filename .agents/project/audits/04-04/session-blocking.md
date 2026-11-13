# 04-04 会话请求锁

固定 Laravel 在 `StartSession` 中解析已解密的会话 Cookie，获取 `session:<id>` 原子锁，再读取会话、执行请求和保存会话。路由可以配置锁的有效期和等待时间，也可使用应用级默认值。复核旧版 `6d1a8192` 的 `session/{backend,cookie,store,database_handler}.rs` 及会话测试。旧版互斥用于局部状态和单次存储操作，本次复用现有会话 ID 校验和 Cache 原子锁。 [Laravel Session Blocking](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/session.md#session-blocking) · [Laravel StartSession](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Session/Middleware/StartSession.php)

- 路由注册回调提供 `block(lock_seconds, wait_seconds)` 和 `withoutBlocking()`。Rust 的 `block` 显式传入两个参数，Laravel 调用可省略参数。全局阻塞默认关闭，等待与锁有效期均默认 10 秒，锁存储默认沿用 Cache 配置。
- `StartSession` 按约 50 毫秒的间隔重试，锁覆盖会话读取至保存。正常和失败路径按所有者释放，取消时用 Tokio 异步尝试释放，锁的有效期提供回收上限。等待超时沿用类型化缓存错误和 HTTP 异常渲染。
- macOS arm64、Rust 1.94.0 下运行 `cargo +1.94.0 test --locked -p openrust-framework --test session_http -- --test-threads=1`，6 项通过。强制启用 MySQL 8.4.13 会话存储和 Redis 8.10.3 锁分支。真实 TCP HTTP 验证同 ID 并发计数依次为 1 和 2、不同 ID 可独立执行、立即超时返回 500 且无会话 Cookie、下游 404 后锁可重新获取、后续请求可正常保存。内存分支覆盖应用级默认和路由 `withoutBlocking()` 的组合。验证后删除临时 MySQL 数据库和受限账户。
- 同步三语会话文档。其余会话驱动、其他数据库及跨平台验证保留在各自验收范围。

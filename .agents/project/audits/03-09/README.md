# 03-09 Redis 连接及缓存、锁接入

## 契约与实现

- 对照固定 Laravel RedisManager、连接测试、RedisStore、RedisLock 和 LuaScripts。参考 Goravel CacheServiceProvider 的应用级服务绑定。复用旧版 `6d1a8192` 的异步 ConnectionManager、Redis 原生命令与所有者 Lua 校验，并沿用旧版清空逻辑库的回归场景。 [Laravel Redis](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/redis.md) · [Laravel Cache](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#atomic-locks) · [Goravel CacheServiceProvider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/cache/service_provider.go)
- RedisManager 按应用缓存命名连接，命令传输按需建立。HTTP、Artisan 和 Cache 复用应用容器中的连接服务。支持字节读写、原始命令和独立订阅连接。原始命令保留 Redis 参数，调用方可通过 `key` 应用前缀。
- 缓存使用 Redis 原生 TTL、NX、批量读写、GETDEL 和整数增减。GETDEL 需要 Redis 6.2 或更高版本。整数使用 Redis 可运算的编码，其他数据使用带标记的 Rust JSON，含字节类型。Redis 锁用毫秒租期和 Lua 所有者校验，数据与锁可使用独立命名连接。清空使用 FLUSHDB，影响选定逻辑库的全部键。
- 连接可为后续命令重建传输，结果不确定的命令交给调用方判断是否安全重试。重连使用客户端的退避实现。Redis Cluster、其他客户端的退避算法、自动命令重放、缓存标签及 Windows、真实 TLS 尚未验收。

## 验收

隔离基线 `21a65e98` 加已验收的依赖与本批改动，macOS arm64，Rust 1.94.0，专用 Redis 8.10.3。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。测试报告 394 项通过。本批未启用真实 MySQL 和 S3。
- 真实 Redis 验证 ACL 认证、URL 与配置的逻辑库选择、Pub/Sub、批量 JSON 与字节、原子 add/pull、计数保留 TTL、跨应用竞争、过期锁、旧持有者失效、进程间所有者恢复。精确终止数据连接后读请求恢复。
- 实际 HTTP 验证缓存命中、Artisan 清空后缺失及独立库的锁保留。三语 Redis 文档示例编译后均通过真实 HTTP。实际运行 `./artisan cache:forget artifact redis`、`./artisan cache:clear redis`、`./artisan inspire`，均成功。
- 文档构建成功，根路径与子路径通过 12 项通用浏览器检查和 6 项 Redis、缓存交叉链接检查。

下一批为 03-10 国际化、回退和复数，依赖已验收的 01-01 配置。

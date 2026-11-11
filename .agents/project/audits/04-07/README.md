# 04-07 命名限流与 HTTP 节流

## 契约与实现

- 对照固定 Laravel 的 RateLimiter、Limit、ThrottleRequests、ThrottleRequestsWithRedis 和响应头规则。参考固定 Goravel 的命名节流中间件，从旧版 `6d1a8192:vendor/openrust/framework/src/core/rate_limit.rs` 及 `http/rate_limit.rs` 提取固定窗口、匿名计数键和 Redis 竞争的验证方式。 [Laravel 限流](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/rate-limiting.md) · [Laravel 路由节流](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md#rate-limiting) · [Goravel Throttle](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/http/middleware/throttle.go)
- 应用独立注册 `RateLimiter::r#for`，用 `throttle:name` 附加命名策略。支持多限制、重复键回退、取消、无限制、直接响应、条件计数和自定义拒绝。429 由框架异常 Handler 协商正文，并附带 Laravel 的额度、剩余、重试和重置响应头。
- 普通缓存路径用有序的逐键锁串行化同键检查与消耗。Redis 路径用一条 Lua 命令对多个配额执行原子检查和计数。两者均在首次消耗时开始固定窗口。Rust 使用 SHA-256 和独立命名空间标识计数键，避免保存明文业务键。缓存驱动须支持原子锁。
- 保留差异：本批接通命名节流别名，数字参数和认证用户签名仍待接入。手动 `attempt` 按 Laravel 顺序在回调成功后计数，带 `after` 的限流器也在响应后计数，均有并发请求先通过检查的窗口。Redis Lua 原子验收使用独立 Redis 实例配置，Redis Cluster 尚未验收。

## 验收

隔离基线 `21a65e98` 加已验收依赖和本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 428 项测试通过。
- 真实 HTTP 覆盖允许、拒绝、429 正文与响应头、窗口重置、最严格响应头、多限流拒绝时不消耗前置配额、排除、条件计数、自定义响应、无限制和手动计数。Redis 8.10.3 跨两个独立 HTTP 进程并发 24 请求，5 个通过、19 个返回 429。
- 三语共六个示例编译并发出真实 HTTP 请求，均验证 5 次通过后被限流。 `./artisan inspire` 正常退出。VitePress 构建、12 项通用浏览器组合和 12 项 Routing/Rate Limiting 正文导航检查通过。

下一批 04-08 异常报告限流，依赖本单元与 01-06 异常处理。

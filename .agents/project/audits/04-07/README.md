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

## 遗漏回归补交

命名限流双应用测试在原验收日志通过，未进入 `cb0e43e6`。在包含路由安装器的 `a6f560e8` 上复验 `named_limiters` 与 `throttling`。真实 HTTP 覆盖两个应用的同名策略隔离、重复键、响应回调及跨进程 Redis 节流。Rust 1.94.0 锁定离线测试、格式及定向严格 Clippy 通过。三语正文沿用原交付。

## 数字节流与认证签名补验

- 在已有数字参数和认证 ID 支持上补齐 `throttle` 的每分钟 60 次默认值、数字参数的命名策略解析优先级、小数分钟和 `访客|认证` 配额。访客按路由声明的域名和连接 IP 计数。认证请求按原始认证标识计数。复核旧版 `http/rate_limit.rs` 的用户或 IP 策略，沿用当前框架固定窗口和原子 Redis 计数。 [Laravel ThrottleRequests](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/Middleware/ThrottleRequests.php#L80-L215)
- macOS arm64、Rust 1.94.0，`cargo test -p openrust-framework --test named_limiters --test throttling --locked --offline` 通过六项。真实 HTTP 覆盖默认额度、访客与认证额度、身份隔离、不同路由前缀、通配域名、不同路由域名、客户端 Host 变化、额度响应头、429、到期恢复和数字名称的命名策略。使用 Redis 8.10.3 复验两个独立 HTTP 进程共享认证与访客的配额。
- 小数分钟转换为整秒，现有后端支持 1 秒至 365 天窗口。Laravel 可从认证模型的动态属性读取配额，Rust 的 `Authenticatable` 接口公开认证标识，按应用属性配置额度可使用命名限流器。Redis Cluster 留到发布前验收。

## 认证模型属性配额

- 对照固定 Laravel `ThrottleRequests::resolveMaxAttempts` 和 Redis 版本的共享参数路径。保留命名限流优先和访客／认证分支，认证分支可读取同名属性。数据库用户提供器直接读取列，应用 Eloquent 模型通过 `Authenticatable::getAuthAttribute` 委托现有属性访问器和转换。自定义认证用户可实现同一接口。额度接受有符号 64 位范围内的非负数和数值字符串，小数向零截断。复核旧版 `http/rate_limit.rs`，沿用认证标识与访客 IP 分桶；旧版没有属性配额接口。 [固定 Laravel 节流中间件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/Middleware/ThrottleRequests.php#L170)
- 在隔离基线 `1c9da1168` 上，以真实 HTTP 复验 Array 和 Redis 8.10.3。锁定测试六项通过，覆盖命名限流仍优先、访客固定额度、数字与字符串属性、缺失或无效属性、不同身份隔离，以及两个独立进程共用额度和 429。真实 MySQL 8.4.13 的会话用例通过，Eloquent 会话用户和数据库 Token 用户均从持久化属性取得额度。
- Rust 1.94.0 全仓格式、严格 Clippy、独立 `./artisan inspire`、三语站点、12 项通用和 6 项路由正文浏览器检查通过。临时 Redis、MySQL 和本批产物已回收，Cargo 清理 3.3 GiB，项目卷可用 80.97 GB（75.41 GiB），周额度剩余 51%。Redis Cluster 多额度同槽尚待验收。证据目录 `/private/tmp/openrust-04-07-model-quota.RwoStlgY`。

## Redis Cluster 多额度与 04-07 整单元验收

- 对照固定 Laravel 的 `ThrottleRequestsWithRedis` 和 `DurationLimiter`。Laravel 用单键脚本处理各项限流。OpenRust 保留一次脚本检查并扣减该命名策略的所有额度，以免因后续额度拒绝而消耗前置额度。复核旧版 `core/rate_limit.rs`、`http/rate_limit.rs` 和真实 Redis 回归，旧版未提供 Cluster 多键计数实现。 [固定 Laravel Redis 节流](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/Middleware/ThrottleRequestsWithRedis.php) · [固定 Laravel 路由文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md#throttling-with-redis)
- 基线 `e451aed47` 在 Redis 8.10.3 集群上的双额度请求复现 HTTP 500。命名策略按名称和连接前缀派生槽标签，槽标签放在用户前缀之前，避免其中的花括号改变 Redis 的槽解析。数值节流及单机物理键保持原格式。已有集群命名计数的旧键按原 TTL 过期，升级开启新窗口。
- 真 Redis 8.10.3 集群上的 `sh tests/redis_cluster.sh` 五项通过。双进程 HTTP 验证多额度计数、后置额度拒绝不消耗前置额度、条件计数、访客和认证模型配额，以及 24 次同时竞争中 5 次通过、19 次返回 429。单机 Redis 和 Array 定向回归通过，并复验旧有的 Redis 跨槽读写、独立锁库、标签清理、迁槽和故障切换。
- Rust 1.94.0 全仓格式、严格 Clippy、独立 `./artisan route:list`、三语站点、12 项通用和 6 项路由到 Redis 配置的浏览器检查通过。临时 Redis 与本批产物已回收，Cargo 清理 3.8 GiB。项目卷可用 81.05 GB（75.49 GiB），周额度剩余 51%。证据目录 `/private/tmp/openrust-04-07-cluster-quota.MgGkhGxn`。

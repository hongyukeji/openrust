# API-F01 异步命名限流安装前置

2026-11-20。Fortify 1.43.0 安装 Provider 的 login 与 passkeys 限流器读取请求体；Rust 的输入解析需要异步可变借用。新增 `RateLimiter::forAsync`／`asyncLimiter`，现有同步注册接口继续使用。

## 来源与实现

对照固定 Framework 13.40.0 的 Cache RateLimiter::for／limiter 及 ThrottleRequests::handleRequestUsingNamedLimiter；Fortify 固定版本的 stubs/FortifyServiceProvider.php。查阅本地 v0.x 的 auth/fortify/throttle.rs：旧版自建同步包装不能读取当前原生异步 Request，未复制该协议。

- 同一应用内一个加锁命名表同时保存同步和异步策略；同名注册原子替换，路由一次查表并持有本次回调快照，等待请求体时不持有注册锁。
- 异步回调返回原有四种 LimiterResponse，也可传播 HttpError；多配额沿用重复 key 规范化、原子准入、条件计数、自定义响应与响应头。缓存和 Redis 后端共用解析入口。
- 同步 limiter 查询仅返回同步回调；asyncLimiter 仅返回异步回调；不匹配的类型返回 None。路由使用内部统一查表，异步命名策略不会落入数字参数解析。
- 这是 Rust 异步输入所需的显式适配；PHP 原协议无需另增 async 方法。

## 验收

独立 archive 源码与专属 target：`/private/tmp/openrust-async-limiter-20261120/`，基线见 source-sha256-async-limiter.json。验收日志 regressions.log／redis.log 全部通过：

- 新增真实 TCP POST／原生 Request 测试，覆盖用户名分桶、读取后输入保持、双应用隔离、四种策略结果、HTTP 错误与坏 JSON、多配额重复 key、条件计数／自定义拒绝；同步与异步同名替换后，既有回调快照仍可执行。
- named_limiters 1 函数、Array throttling 4 函数、验证器32函数、Fortify49函数在 MySQL8.4.13 验收入口下通过；真实 Redis8.10.3 新异步和原有配额／认证签名共3函数，通过两个独立 HTTP 进程共享配额。
- Rust1.94 workspace／all-targets features pail,passport,fortify 严格 Clippy、全仓 fmt、根 main 构建及 HTTP／Pail／恢复／超时清理／SIGINT／端口释放／独立 Artisan 全过。
- 本批未重新执行 Redis Cluster 和完整发布矩阵；Redis／cache 计数后端未修改。6份输入与快照逐字节一致，专属 target 闲置核对后删除，Available释放6,873,100 KiB、剩余92,065,964 KiB；源码／日志及全部卷保留。见 cleanup-async-limiter.json。

初轮编译修正闭包的高阶生命周期推断；随后重试曾误从 live 目录启动 Cargo，发现后立即中止，专属 target 仍保持隔离；正式验收明确从独立源码快照执行。保留 test.log／retry.log／retry-snapshot.log 诊断日志。

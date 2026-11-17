# 04-03 中间件顺序、别名和 CORS

## 契约与实现

- 对照固定 Laravel 的路由中间件展开、排序、排除、HTTP Kernel 终止阶段、HandleCors 与 HTTP 集成用例。参考 Goravel 的编译型中间件注册。旧版 `6d1a8192` 的 `src/http/middleware.rs` 提供 Origin 校验，`tests/http/router.rs` 和 `tests/unit/foundation/terminable_middleware.rs` 提供错误响应和路由终止回归。 [Laravel Middleware](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/middleware.md) · [Laravel CORS](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md#cors) · [Laravel HandleCorsTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Http/Middleware/HandleCorsTest.php) · [Goravel Middleware](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/configuration/middleware.go)
- 全局中间件包裹路由匹配。解析路由组和别名后按实例排除和去重，再按类型确定路由顺序。终止阶段使用同一个已注册实例，按解析后的路由顺序执行，再执行全局回调。Rust 通过显式注册识别终止能力。回调在传输层释放响应正文后调度，服务正常退出时等待回调任务。
- `HandleCors` 在内核启动时加载应用级配置。支持通配路径、按主机覆盖、固定来源、通配和正则来源及请求头协商。匹配的预检返回空正文 204。实际响应经路由或异常处理后补充 CORS 头，并保留已有 Vary。Rust 在启动时编译策略并拒绝无效配置。
- 差异：正则按 Rust 引擎能力执行。全 URL 匹配以请求 URI 和 Host 为依据，代理转发的协议和主机信任尚未接入。Laravel HandleCors 的进程级 `skipWhen` 与 `flushState` 未移植。终止时点是传输层释放正文，涵盖客户端断开。流结束及断开已实测，带长回调的进程级优雅关闭未独立验收。默认 Web 中间件组的 Cookie、Session、CSRF 和错误共享顺序属于 04-04。

## 验收

隔离基线 `21a65e98` 加已验收依赖与本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，406 项测试通过。
- TCP 验收覆盖跨源预检、拒绝授权、普通 OPTIONS、匹配 404、Vary 去重、主机覆盖、两个应用的策略隔离、启动失败重试、路由排除、别名去重、执行优先级、并行请求、流结束和中途断开。直接调用内核的终止接口可重复调用，同一个响应的回调只执行一次。
- 隔离副本的应用进程运行三语 CORS curl 示例均为 HTTP 204，`GET /up` 分别返回预期 JSON 和 HTML，`./artisan inspire` 退出 0。
- 三语文档站构建成功，根路径和子路径共 12 项浏览器通用组合、12 项 Middleware 和 Routing 正文专项通过。VitePress 报告已有的包体积提示。

下一批为 04-04 Cookie、Session 与 CSRF，依赖 02-06、03-04、03-06 和本批中间件生命周期。数据库会话须通过真实 MySQL 8.4 验收。

开始 04-04 后联合复跑上述 Rust 三项检查，407 项测试通过，新增一项 Cookie 名称绑定前缀对照。04-04 的 HTTP 行为尚未验收。

## 2026-11-12 遗漏回归补交

路由可选参数和约束、真实 HTTP 中间件优先级、短路、404/405、HEAD 和注册顺序，新增 8 项。 与同文件其他单元按测试项拆分。隔离副本用 Rust 1.94.0 锁定离线运行当前分块测试及严格 Clippy，均通过。三语正文沿用原提交。

## 可信代理与转发 HTTPS

- 对照固定 Laravel `TrustProxies`、Symfony 7.4.0 请求与旧版 `v0.x` 的 `proxy.rs`、`client_ip.rs` 和请求 URL 对照夹具。复用头部位掩码、按可信链过滤客户端地址、CIDR 与带引号的 `Forwarded` 解析思路。信任策略由 `withMiddleware` 在应用级装配，请求发布自己的代理元数据。 [Laravel 可信代理](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#configuring-trusted-proxies) · [Laravel TrustProxies](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Middleware/TrustProxies.php)
- 只有可信 socket 对端的启用字段可影响请求。URL、CORS、Cookie 安全属性、访客限流和数据库会话使用处理后的协议、来源或客户端地址。外部前缀进入 URL 生成，路由匹配与原始头部读取保持各自职责。内置 HTTP listener 显式标记明文传输，外部 TLS 适配器可注入 `RequestSecurity`。
- Rust 对启用头的冲突在请求入口返回 400，并将转发端口限制为有效 `u16` 端口。平台 Host 后缀自动开启信任、`trustedproxy` 配置文件回退和可信 Host 白名单继续跟踪。当前默认需要应用显式配置代理地址。
- 真实 TCP 与 Symfony/PHP 8.5 对照验证 IPv4 链、IPv6、HTTPS、端口、前缀、可信和非可信对端及冲突拒绝。04-03 保持开放，后续核对可信 Host 和部署配置的剩余边界。
- 隔离基线 `0c02b34c` 使用 Rust 1.94.0，通过应用 HTTP 16 项、可信代理 2 项、会话 HTTP 8 项、URL 4 项、实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、三语站点和 18 项浏览器检查。全仓格式仅报告既有 `sanctum_authentication.rs:1412`。验收日志保存于 `/private/tmp/openrust-04-03-trusted-proxy.nkwaRr69`。

## 可信 Host 与部署配置

- 对照固定 Laravel `TrustHosts` 和 `TrustProxies`，复用 `v0.x` 的 PCRE Host 模式、应用域名回退和请求局部校验。`trustHosts` 编译静态模式，`trustHostsWith` 允许在当前应用作用域按请求异步解析。模式按主机名匹配，区分端口和主机的解析边界。在 `local` 与 `testing` 跳过校验。 [Laravel 可信 Host](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#configuring-trusted-hosts)
- 入口按同一请求快照校验原始 Host 和有效转发 Host，失败在路由前返回 400。静态模式在装配时编译，动态模式回调错误交给异常处理器。启用子域时从 `app.url` 追加应用主机及子域匹配。
- 应用装配未设置代理策略时读取 `trustedproxy.proxies` 与可选 `trustedproxy.headers`。后者复用 Laravel 的位掩码和预设语义作为 OpenRust 配置扩展。显式空代理列表优先于配置回退，便于停用转发头信任。托管入口仍要求明确的代理地址配置，任意 Host 字符串不会启用代理信任。
- 真实 TCP 与固定 PHP 8.5 对照通过 PCRE 前瞻、大小写、应用子域、异步模式、环境跳过、AWS 头预设及配置回退。真实请求另验证原始 Host 即使带允许的转发 Host 仍受校验，以及空代理策略覆盖回退。下一批核对 CORS 条件跳过和长终止回调下的优雅退出。
- 隔离基线 `201519cde` 使用 Rust 1.94.0，通过应用 HTTP 16 项、可信代理与 Host 5 项、会话 HTTP 8 项、URL 4 项、实际 `route:list --json --method=HEAD` 与 `artisan inspire`、严格 Clippy、三语站点和 18 项浏览器检查。全仓格式仍仅报告既有 `sanctum_authentication.rs:1412`。验收日志保存于 `/private/tmp/openrust-04-03-trusted-hosts.5ht4Maip`。

## CORS 条件跳过与整单元验收

- 固定 Laravel 的 `HandleCors::skipWhen` 在匹配路径和处理预检前按注册顺序求值。Rust 在 `withMiddleware` 中提供应用级 `skipCorsWhen`，同进程各应用独立持有回调。即使没有启用 CORS 路径，注册的回调仍运行。沿用旧版 `v0.x` 的 Origin 与传输层回归场景，旧版 `CorsLayer` 无对应的条件跳过接口。 [Laravel HandleCors](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Middleware/HandleCors.php)
- 真实 TCP 验证普通预检 204、跳过后路由 404、实际请求 200、应用间隔离与原有响应头保留。CORS 错误响应、来源拒绝、按主机覆写、无路径及无效配置继续覆盖。真实子进程在收到 SIGINT 后关闭监听口，等待已启动的终止回调。释放回调后验证恰好执行一次并以状态 0 退出。流式正文结束与客户端断开沿用现有回归。 [Laravel 终止中间件](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/middleware.md#terminable-middleware)
- 隔离基线 `3fa7d3847`，macOS arm64 与 Rust 1.94.0。中间件、维护页与排除共 8 项、应用 HTTP 16 项、可信代理与 Host 5 项通过。根应用三语 curl 示例通过实际 HTTP 预检，`./artisan inspire` 和 `./artisan route:list --json --method=HEAD` 均通过。工作区严格 Clippy、改动文件格式、VitePress 三语及 24 项浏览器检查通过。
- 扩展执行的 `tests/startup.rs` 共 24 项通过，Provider 并发生成和 stub 数量两项在本批及未修改的基线上均失败，移交 Artisan 回归单独跟踪。全仓格式仍只报告既有 `sanctum_authentication.rs:1412`。本批回收 4.3 GiB 隔离 Cargo 产物与文档缓存，日志保留于 `/private/tmp/openrust-04-03-cors.xPTK6F2C`。
- 04-03 的计划验收项闭合。OpenRust 显式配置可信代理并在应用级隔离 CORS 回调。Windows 信号及托管入口的部署验证留在发布验收范围。

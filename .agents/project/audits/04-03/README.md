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

# 06-04 启动前维护页面

对照 Laravel [预渲染维护页](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/configuration.md#pre-rendering-the-maintenance-mode-view)、[维护入口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/stubs/maintenance-mode.stub)、[错误视图入口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Exceptions/RegisterErrorViewPaths.php)及[集成测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Foundation/MaintenanceModeTest.php)。固定 Goravel 在应用内中间件处理维护响应。v0.x 无可提取的启动前维护页面。

文件维护状态有预渲染内容时，原生 HTTP 监听器在配置加载成功后即能返回快照。请求期望 JSON、命中排除路径或使用有效绕过时，等应用内核接管。Provider 启动失败时继续提供 HTML，其他请求返回 500，解除维护后以启动错误退出。缓存维护使用正常运行的内核。Artisan 的 `--render=errors::503` 优先读取应用的 `resources/views/errors/503.blade.html`，缺省时渲染框架内置错误页面。Rust 的 HTTP 进程需要先获得本机监听配置。

## 验收

- 真实 TCP 通过两项启动夹具。Provider 阻塞时提供预渲染 503、Retry-After、HEAD 和浏览器跳转，排除路径的 JSON 请求等待内核。成功启动后同一监听器接管，解除维护后恢复普通响应。Provider 失败时继续提供预渲染页，内核请求返回 500，清除维护后以启动错误退出。
- 实际 Artisan 验证内置 `errors::503`、应用视图覆盖和 Redis 双进程维护状态。三语 `./artisan down --render=errors::503` 与恢复命令实跑通过。
- Rust 1.94.0 格式、严格 Clippy 和含 SQLite、MySQL 8.4、Redis 8 的锁定离线工作区通过，585 项通过、7 项忽略。三语文档构建及 18 项浏览器检查通过。

## 启动前维护页面的请求目标匹配

- 固定 Laravel 预渲染入口用原始请求目标判断绕过 URL 与跳转目标，运行后的维护中间件使用去掉两端斜杠的路径。OpenRust 原生监听器复用 HTTP URI 中的路径和查询串，保留现有本地重定向约束。复核 openrust-v0.x 未提供启动前维护入口。 [Laravel 预渲染入口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/stubs/maintenance-mode.stub#L50) · [Laravel 维护中间件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Http/Middleware/PreventRequestsDuringMaintenance.php#L70)
- 在隔离快照 `3d99aefd6` 先用真实 TCP 复现带查询串的目标在内核启动前返回 503，修正后返回 302。覆盖尾斜杠、查询串、带查询串的绕过目标、HEAD、Provider 阻塞和失败、内核接管后匹配路径。实际 Artisan 的 `down`、`up` 验证状态、自定义 HTML、签名绕过与 Redis 8.10.3 双进程共享状态。
- 全仓格式、8 项定向回归、严格 Clippy、三语构建和 18 项浏览器检查通过。Redis 临时实例与本批生成文件已清理，卷清单一致。剩余磁盘 82.96 GB（77.26 GiB）。提交前周额度剩余 24%，第二窗口无读数，按用户授权继续。日志保留于 `/private/tmp/openrust-maintenance-paths.whXGZt5W`。

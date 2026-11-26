# 链式路由注册

- 固定 Laravel v13.40.0 的 [Route 门面](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades/Route.php) 将单条路由、资源待注册对象和路由组配置交给各自的公开入口。[PendingResourceRegistration](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Routing/PendingResourceRegistration.php) 在执行注册前收集配置，支持显式注册和析构注册。
- 固定 [Goravel v1.18.0 路由契约](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/route/route.go) 同样将单条路由的命名和分组配置留在注册阶段。Axum 的 [Router](https://docs.rs/axum/0.8.9/axum/struct.Router.html#method.route) 和 Actix Web 的 [ServiceConfig](https://docs.rs/actix-web/4.20.0/actix_web/web/struct.ServiceConfig.html#method.route) 也将应用注册与 HTTP 响应构造分开。
- OpenRust 旧版 `6d1a81924` 的 `vendor/openrust/framework/src/routing/router.rs` 用消费式 Router 构建器配置名称和组。主线已在作用域内持有 Router，因此复用主线的路由、资源控制器及组栈配置。
- 单条注册立即写入 Router，返回句柄供链式配置。资源在链式语句结束时注册，自动注册错误由当前路由注册作用域传播。调用 `register()` 可在当前表达式处理资源注册错误。已有的 `?;` 注册语句继续有效，直接返回原注册结果的回调改为显式返回 `Ok(())`。

## 验收

- Rust 1.95.0 的 `cargo check --locked --workspace --tests` 检查工作区测试目标。欢迎页和框架请求测试合计 68 项，覆盖链式资源注册、路由组前缀和中间件。
- `artisan route:list --json` 确认 `GET|HEAD /` 名为 `home`，处理器是 `HomeController::index`。独立进程的实际 HTTP 请求确认欢迎页 200、HEAD 空响应体、`/api` 仍返回 JSON，以及配置首页跳转时的 302 和原始查询参数。

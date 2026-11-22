# 中间件

在 `bootstrap/app.rs` 的 `withMiddleware` 闭包中配置全局中间件，使用 `append` 追加或 `prepend` 前置。用 `RouteGroup::middleware` 为路由组设置中间件，用 `Route::middleware` 为单条路由设置中间件。

全局中间件包裹路由匹配过程，也能读取 404 和 405 响应。匹配路由后，先执行外层路由组的中间件，再执行内层路由组和单条路由的中间件。每一层都可以调用 `next.run(request).await` 继续执行并读取返回的响应，也可以直接返回响应，跳过其余中间件和路由处理器。

## 默认全局语言中间件 {#locale-middleware}

默认全局栈包含 `openrust::http::middleware::SetLocale`，无需应用注册。候选配置、关闭识别、执行位置与响应头行为见[浏览器语言](../digging-deeper/localization.md#welcome-page-language)。 URL 参数和配置方式见[URL 语言选择](../digging-deeper/localization.md#locale-query)。

## 命名组与优先级

在 `withMiddleware` 中用 `group` 注册命名组，用 `alias` 注册单个实例。组成员可以是 `MiddlewareEntry::Instance`，也可以用 `MiddlewareEntry::named` 引用另一个组或别名。通过 `middleware_name` 将组名或别名附加到路由或路由组。名称缺失或引用成环会使应用在监听前启动失败。

用 `priority` 传入按执行顺序排列的具体中间件类型 ID，例如 `TypeId::of::<Authenticate>()`。框架先展开组，再将顺序颠倒的高优先级项移到它前面遇到的低优先级项之前。未列入优先级的条目保留彼此的相对顺序。同一个实例经多次引用时只执行一次。优先级作用于路由中间件链。全局中间件按配置顺序在路由链外层运行。

在路由或路由组上用 `withoutMiddleware("name")` 排除指定的别名或组。排除也适用于通过嵌套组引用的同一实例。全局中间件仍会执行。

## 可终止中间件

实现 `TerminableMiddleware`，在 HTTP 传输层释放响应正文后取得请求和响应的元数据。在 `withMiddleware` 中用 `appendTerminable`、`prependTerminable` 或 `aliasTerminable` 注册。向组内加入具体实例时用 `MiddlewareEntry::terminable`。

内核先按解析后的顺序调用匹配路由的终止回调，再调用全局回调。请求处理与终止阶段使用同一个已注册实例。传输层消费流式正文期间，终止回调会等待。客户端断开并且传输层释放正文时也会执行回调。

直接调用 `Kernel::handle` 的应用应在传输结束后，用请求和响应部件调用 `Kernel::terminate`。内置 HTTP 服务会自动处理。

优雅退出时，内置 HTTP 服务停止接受新连接，等待现有连接和终止回调结束后再退出。

## 默认 Web 和 API 组

配置会话驱动后，默认 `web` 组依次运行 `EncryptCookies`、`AddQueuedCookiesToResponse`、`StartSession`、`ShareErrorsFromSession`、`PreventRequestForgery` 和 `SubstituteBindings`。默认 `api` 组运行 `SubstituteBindings`。

### API 请求限流

默认 `api` 组不启用限流。先按[路由限流](routing.md#定义限流器)在服务提供者的 `boot` 方法中注册命名策略，再在 `withMiddleware` 中启用：

```rust
.withMiddleware(|middleware| {
    middleware.throttleApi(Some("api"), false);
})
```

参数须显式传入：`Some("api")` 选择 `api` 策略，`false` 保持当前后端。传入其他名称可选择对应策略，传入 `None` 可关闭默认 API 组内的限流。多次调用时使用最后指定的策略；此方法不会代替应用注册策略。

第二个参数传入 `true` 时会调用 `throttleWithRedis()`，将 API 及其他路由的 `throttle` 别名统一切换为 Redis。后续传入 `false` 不会撤销该切换。

默认 API 组依次执行 `statefulApi()` 启用的有状态会话中间件、限流中间件和 `SubstituteBindings`。通过 `group("api", ...)` 显式替换组时，以自定义组为准，不受 `throttleApi` 调用顺序影响。

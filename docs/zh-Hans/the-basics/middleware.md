# 中间件

在 `bootstrap/app.rs` 的 `withMiddleware` 闭包中配置全局中间件，使用 `append` 追加或 `prepend` 前置。用 `RouteGroup::middleware` 为路由组设置中间件，用 `Route::middleware` 为单条路由设置中间件。

全局中间件包裹路由匹配过程，也能读取 404 和 405 响应。匹配路由后，先执行外层路由组的中间件，再执行内层路由组和单条路由的中间件。每一层都可以调用 `next.run(request).await` 继续执行并读取返回的响应，也可以直接返回响应，跳过其余中间件和路由处理器。

## 命名组与优先级

在 `withMiddleware` 中用 `group` 注册命名组，用 `alias` 注册单个实例。组成员可以是 `MiddlewareEntry::Instance`，也可以用 `MiddlewareEntry::named` 引用另一个组或别名。通过 `middleware_name` 将组名或别名附加到路由或路由组。名称缺失或引用成环会使应用在监听前启动失败。

用 `priority` 传入按执行顺序排列的具体中间件类型 ID，例如 `TypeId::of::<Authenticate>()`。框架先展开组，再将顺序颠倒的高优先级项移到它前面遇到的低优先级项之前。未列入优先级的条目保留彼此的相对顺序。同一个实例经多次引用时只执行一次。优先级作用于路由中间件链。全局中间件按配置顺序在路由链外层运行。

在路由或路由组上用 `withoutMiddleware("name")` 排除指定的别名或组。排除也适用于通过嵌套组引用的同一实例。全局中间件仍会执行。

## 可终止中间件

实现 `TerminableMiddleware`，在 HTTP 传输层释放响应正文后取得请求和响应的元数据。在 `withMiddleware` 中用 `appendTerminable`、`prependTerminable` 或 `aliasTerminable` 注册。向组内加入具体实例时用 `MiddlewareEntry::terminable`。

内核先按解析后的顺序调用匹配路由的终止回调，再调用全局回调。请求处理与终止阶段使用同一个已注册实例。传输层消费流式正文期间，终止回调会等待。客户端断开并且传输层释放正文时也会执行回调。服务优雅关闭时会等待未完成的回调。

直接调用 `Kernel::handle` 的应用应在传输结束后，用请求和响应部件调用 `Kernel::terminate`。内置 HTTP 服务会自动处理。

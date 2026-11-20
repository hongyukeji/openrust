# 开发者文档边界整理：docs/zh_CN/architecture-concepts/facades.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/architecture-concepts/facades.md (迁移前)
+++ docs/zh_CN/architecture-concepts/facades.md (开发者正文)
@@ -29,7 +29,7 @@

 框架为应用引导、每个 HTTP 请求和每次 Artisan 调用设置门面作用域。同一应用中的嵌套执行共享作用域，不同应用彼此隔离。新请求或命令使用新的门面缓存；注册为单例的服务仍属于该应用的容器。应用代码自行创建的 Tokio 任务不会自动继承门面作用域，应向这些任务传入已解析服务。

-`Route::get` 与 `Artisan::command` 在路由模块的 `register()` 执行期间使用专用注册作用域，属于已交付的注册门面。其他门面类型声明存在，不代表对应服务已经实现。Mocking 辅助接口和解析回调由后续单元交付。
+`Route::get` 与 `Artisan::command` 在路由模块的 `register()` 执行期间使用专用注册作用域。服务门面须在应用生命周期内使用，并先注册对应服务。

 未加载的延迟 Provider 或异步工厂使用 `getFacadeRootAsync().await`。它直接走一次异步容器解析，等待就绪并共用当前应用的门面缓存；期间出现较新的 `swap` 或其他已缓存根对象时保留该值。同步解析未就绪的 Provider 或尚未构造的异步工厂返回类型化的异步解析错误。

````

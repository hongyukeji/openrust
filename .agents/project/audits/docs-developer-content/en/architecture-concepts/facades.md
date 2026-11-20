# 开发者文档边界整理：docs/en/architecture-concepts/facades.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/architecture-concepts/facades.md (迁移前)
+++ docs/en/architecture-concepts/facades.md (开发者正文)
@@ -29,7 +29,7 @@

 The framework scopes facade state to bootstrap, each HTTP request and each Artisan invocation. Nested execution in the same application shares the scope; different applications remain isolated. A new request or command starts a fresh facade cache. A service registered as a singleton still belongs to its application's container. Tokio tasks created by application code do not automatically inherit an ambient facade scope; pass resolved services into those tasks.

-`Route::get` and `Artisan::command` use dedicated registration scopes while the route modules' `register()` functions execute. These are the delivered registration facades. The presence of other facade type declarations does not imply their backing services are implemented. Mocking helpers and resolution callbacks are delivered by later units.
+`Route::get` and `Artisan::command` use dedicated registration scopes while the route modules' `register()` functions execute. Resolve service facades within an application lifecycle after registering their backing services.

 Use `getFacadeRootAsync().await` for an unloaded deferred provider or an asynchronous factory. It resolves once through the asynchronous container path, waits for readiness and shares the application's facade cache. A newer `swap` or concurrently cached root is retained. Synchronous lookup of a pending provider or uncached async factory returns a typed async-required error.

````

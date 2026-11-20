# 开发者文档边界整理：docs/en/packages/folio.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/packages/folio.md (迁移前)
+++ docs/en/packages/folio.md (开发者正文)
@@ -8,8 +8,8 @@

 ## Available entry points {#available-entry-points}

-See [Routing](../the-basics/routing.md) for the native interfaces and their tested scope. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.
+See [Routing](../the-basics/routing.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

 ## Scope {#scope}

-There is no verified installation command for the missing integration. Do not copy a Laravel Composer or Artisan command into OpenRust on the assumption that this navigation entry implements it. Future ports require their own public API, configuration, tests and three-language documentation.
+OpenRust does not provide an installation command for this integration. Do not copy a Laravel Composer or Artisan command into OpenRust on the assumption that this navigation entry implements it.
````

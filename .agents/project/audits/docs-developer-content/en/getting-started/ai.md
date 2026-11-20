# 开发者文档边界整理：docs/en/getting-started/ai.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/getting-started/ai.md (迁移前)
+++ docs/en/getting-started/ai.md (开发者正文)
@@ -2,14 +2,14 @@

 ## Support status {#support-status}

-**Partial native support.** The repository contains development guidance in AGENTS.md and .agents/project/README.md. It does not include a framework AI agent runtime or an installer for AI services.
+**Not provided.** OpenRust does not include a framework AI agent runtime or an installer for AI services. Integrate an external AI service through your application code.

 This chapter retains the Laravel navigation path for comparison. Its presence does not install a package or promise compatibility with the corresponding PHP API.

 ## Available entry points {#available-entry-points}

-See [Contribution Guide](../prologue/contributions.md) for the native interfaces and their tested scope. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.
+See [Contribution Guide](../prologue/contributions.md) for the native interfaces and usage. When integrating a separate library or service, implement the application adapter and validate its lifecycle, errors and credentials against that service's contract.

 ## Scope {#scope}

-There is no verified installation command for the missing integration. Do not copy a Laravel Composer or Artisan command into OpenRust on the assumption that this navigation entry implements it. Future ports require their own public API, configuration, tests and three-language documentation.
+OpenRust does not provide an installation command for this integration. Do not copy a Laravel Composer or Artisan command into OpenRust on the assumption that this navigation entry implements it.
````

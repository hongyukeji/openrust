# 开发者文档边界整理：docs/zh_CN/packages/dusk.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/packages/dusk.md (迁移前)
+++ docs/zh_CN/packages/dusk.md (开发者正文)
@@ -2,14 +2,14 @@

 ## 支持状态 {#support-status}

-**部分原生支持。** 文档及部分认证流程使用 Playwright 做浏览器验收。没有 Dusk 包、WebDriver 管理器或 dusk Artisan 命令。
+**未提供。** OpenRust 没有 Dusk 包、WebDriver 管理器或 `dusk` Artisan 命令。可使用外部浏览器测试工具测试正在运行的应用。

 本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

 ## 可用入口 {#available-entry-points}

-原生接口及其已验证范围见 [HTTP Tests](../testing/http-tests.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。
+原生接口及用法见 [HTTP Tests](../testing/http-tests.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

 ## 范围 {#scope}

-缺失集成没有已验证的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。未来移植需分别核定公开 API、配置、测试和三语正文。
+OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。
````

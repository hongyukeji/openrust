# 开发者文档边界整理：docs/zh_CN/getting-started/ai.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/getting-started/ai.md (迁移前)
+++ docs/zh_CN/getting-started/ai.md (开发者正文)
@@ -2,14 +2,14 @@

 ## 支持状态 {#support-status}

-**部分原生支持。** 仓库的 AGENTS.md 与 .agents/project/README.md 提供开发指引；框架没有 AI agent 运行时或 AI 服务安装器。
+**未提供。** OpenRust 没有框架 AI agent 运行时或 AI 服务安装器。应用可通过自身代码集成外部 AI 服务。

 本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

 ## 可用入口 {#available-entry-points}

-原生接口及其已验证范围见 [Contribution Guide](../prologue/contributions.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。
+原生接口及用法见 [Contribution Guide](../prologue/contributions.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

 ## 范围 {#scope}

-缺失集成没有已验证的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。未来移植需分别核定公开 API、配置、测试和三语正文。
+OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。
````

# 开发者文档边界整理：docs/zh_TW/getting-started/ai.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/getting-started/ai.md (迁移前)
+++ docs/zh_TW/getting-started/ai.md (开发者正文)
@@ -2,14 +2,14 @@

 ## 支援狀態 {#support-status}

-**部分原生支援。** 儲存庫的 AGENTS.md 與 .agents/project/README.md 提供開發指引；框架沒有 AI agent 執行環境或 AI 服務安裝器。
+**未提供。** OpenRust 沒有框架 AI agent 執行環境或 AI 服務安裝器。應用程式可透過自身程式碼整合外部 AI 服務。

 本章保留 Laravel 導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

 ## 可用入口 {#available-entry-points}

-原生介面及其已驗證範圍見 [Contribution Guide](../prologue/contributions.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。
+原生介面及用法見 [Contribution Guide](../prologue/contributions.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

 ## 範圍 {#scope}

-缺少的整合沒有已驗證的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。未來移植需分別核定公開 API、設定、測試和三語正文。
+OpenRust 不提供此整合的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。
````

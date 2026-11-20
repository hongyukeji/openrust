# 开发者文档边界整理：docs/zh_TW/ai/ai-sdk.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/ai/ai-sdk.md (迁移前)
+++ docs/zh_TW/ai/ai-sdk.md (开发者正文)
@@ -8,8 +8,8 @@

 ## 可用入口 {#available-entry-points}

-原生介面及其已驗證範圍見 [HTTP Client](../digging-deeper/http-client.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。
+原生介面及用法見 [HTTP Client](../digging-deeper/http-client.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

 ## 範圍 {#scope}

-缺少的整合沒有已驗證的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。未來移植需分別核定公開 API、設定、測試和三語正文。
+OpenRust 不提供此整合的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。
````

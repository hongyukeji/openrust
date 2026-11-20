# 开发者文档边界整理：docs/zh_TW/packages/dusk.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/packages/dusk.md (迁移前)
+++ docs/zh_TW/packages/dusk.md (开发者正文)
@@ -2,14 +2,14 @@

 ## 支援狀態 {#support-status}

-**部分原生支援。** 文件及部分認證流程使用 Playwright 做瀏覽器驗收。沒有 Dusk 套件、WebDriver 管理器或 dusk Artisan 命令。
+**未提供。** OpenRust 沒有 Dusk 套件、WebDriver 管理器或 `dusk` Artisan 命令。可使用外部瀏覽器測試工具測試正在執行的應用程式。

 本章保留 Laravel 導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

 ## 可用入口 {#available-entry-points}

-原生介面及其已驗證範圍見 [HTTP Tests](../testing/http-tests.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。
+原生介面及用法見 [HTTP Tests](../testing/http-tests.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

 ## 範圍 {#scope}

-缺少的整合沒有已驗證的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。未來移植需分別核定公開 API、設定、測試和三語正文。
+OpenRust 不提供此整合的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。
````

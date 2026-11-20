# 瀏覽器測試／Dusk

## 支援狀態 {#support-status}

**部分原生支援。** 文件及部分認證流程使用 Playwright 做瀏覽器驗收。沒有 Dusk 套件、WebDriver 管理器或 dusk Artisan 命令。

本章保留 Laravel 導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

## 可用入口 {#available-entry-points}

原生介面及其已驗證範圍見 [HTTP Tests](../testing/http-tests.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

## 範圍 {#scope}

缺少的整合沒有已驗證的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。未來移植需分別核定公開 API、設定、測試和三語正文。

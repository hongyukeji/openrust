# Telescope

## 支援狀態 {#support-status}

**目前原始碼未提供。** 尚未提供 Telescope 面板、watcher 或 Telescope 儲存結構。原生日誌與 Pail 診斷遵循各自文件範圍。

本章保留 Laravel 導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

## 可用入口 {#available-entry-points}

原生介面及用法見 [Logging](../the-basics/logging.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

## 範圍 {#scope}

OpenRust 不提供此整合的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。

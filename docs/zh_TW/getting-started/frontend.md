# 前端

## 支援狀態 {#support-status}

**部分原生支援。** 已有伺服器端 Blade 視圖、公共檔案與 JSON 回應。目前原始碼沒有 Vue／React 應用程式骨架、Inertia 配接器或前端套件管理工作流程。

本章保留 Laravel 導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

## 可用入口 {#available-entry-points}

原生介面及用法見 [Views](../the-basics/views.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

## 範圍 {#scope}

OpenRust 不提供此整合的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。

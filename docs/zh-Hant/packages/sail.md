# Sail

## 支援狀態 {#support-status}

**目前原始碼未提供。** 儲存庫提供 Docker／Compose 部署檔案，沒有 Sail 包裝器或 sail 命令。請按實際容器裝配與持久化儲存說明使用。

本章保留框架導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

## 可用入口 {#available-entry-points}

原生介面及用法見 [Deployment](../getting-started/deployment.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

## 範圍 {#scope}

OpenRust 不提供此整合的安裝命令。不要因存在此導航入口就將框架Composer 或 Artisan 命令照搬至 OpenRust。

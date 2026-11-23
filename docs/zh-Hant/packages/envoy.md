# Envoy

## 支援狀態 {#support-status}

**目前原始碼未提供。** 尚未提供 Envoy 工作檔案剖析器或遠端部署命令。原生程序與並行服務可執行應用程式自己的命令，不是 Envoy 工作執行環境。

本章保留框架導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

## 可用入口 {#available-entry-points}

原生介面及用法見 [Processes](../digging-deeper/processes.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

## 範圍 {#scope}

OpenRust 不提供此整合的安裝命令。不要因存在此導航入口就將框架Composer 或 Artisan 命令照搬至 OpenRust。

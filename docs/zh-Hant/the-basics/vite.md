# 資源建構／Vite

## 支援狀態 {#support-status}

**部分原生支援。** Blade 支援文件所述的 @fonts 消費 version 1 Vite 字型 manifest。尚未提供完整 Vite 整合、JavaScript／CSS 入口 manifest 解析器或前端開發伺服器生命週期。

本章保留框架導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

## 可用入口 {#available-entry-points}

原生介面及用法見 [Blade Templates](blade.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

## 範圍 {#scope}

OpenRust 不提供此整合的安裝命令。不要因存在此導航入口就將框架Composer 或 Artisan 命令照搬至 OpenRust。

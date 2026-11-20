# Cashier（Stripe）

## 支援狀態 {#support-status}

**目前原始碼未提供。** 尚未提供 Cashier 計費模型、Stripe 訂閱生命週期、Webhook 簽章處理或付款 UI 整合。

本章保留 Laravel 導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

## 可用入口 {#available-entry-points}

原生介面及其已驗證範圍見 [HTTP Client](../digging-deeper/http-client.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

## 範圍 {#scope}

缺少的整合沒有已驗證的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。未來移植需分別核定公開 API、設定、測試和三語正文。

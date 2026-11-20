# 入門套件

## 支援狀態 {#support-status}

**目前原始碼未提供。** 尚未提供現成套件 UI、控制台頁面或 React／Vue／Livewire 套件。原生 init 精靈與 Fortify 提供應用裝配和無介面認證，頁面由應用程式自行實作。

本章保留 Laravel 導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

## 可用入口 {#available-entry-points}

原生介面及其已驗證範圍見 [Fortify](../packages/fortify.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

## 範圍 {#scope}

缺少的整合沒有已驗證的安裝命令。不要因存在此導航入口就將 Laravel Composer 或 Artisan 命令照搬至 OpenRust。未來移植需分別核定公開 API、設定、測試和三語正文。

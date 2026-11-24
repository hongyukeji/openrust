# 入門套件

## 支援狀態 {#support-status}

**完整安裝預設與 UI 尚未提供。** 尚未提供現成套件 UI、控制台頁面或 React／Vue／Livewire 套件。原生 init 精靈與 Fortify 提供應用裝配和無介面認證，頁面由應用程式自行實作。

本章保留框架導航路徑供對照；存在導航入口不代表已經安裝相應套件，也不承諾對應 PHP API 相容。

## 可用入口 {#available-entry-points}

原生介面及用法見 [Fortify](../packages/fortify.md)。整合其他函式庫或服務時，應用程式需實作配接層，並按該服務的契約驗證生命週期、錯誤與憑證處理。

## 範圍 {#scope}

OpenRust 不提供此整合的安裝命令。不要因存在此導航入口就將框架Composer 或 Artisan 命令照搬至 OpenRust。

## 可選 JSON 回應 {#json-responses}

完整套件安裝預設與 UI 尚未交付。已配置好 Fortify Action 與路由的應用程式，可明確使用回應配接。

在 Fortify Provider 之後註冊 `openrust_fortify::starter::StarterKitServiceProvider`。HTML 請求保留原生重新導向與快閃訊息；JSON 登入、註冊及雙因素登入完成後，透過 `ApiResponse` 回傳目前使用者，註冊使用201。更新省略空資料，登出與信箱驗證保留204空本文。訊息依目前應用語言顯示。

```rust
Box::new(openrust_fortify::starter::StarterKitServiceProvider),
```

預設 `UserResource` 只讀取 `id`、`name`、`email`、`email_verified_at`，保留字串主鍵，不列舉模型屬性或讀取憑證。應用程式可替換原有 Fortify 回應綁定來使用自己的資源。認證成功回應設定 `Cache-Control: no-store`。此配接不安裝頁面、不變更認證守衛，也不簽發個人權杖。

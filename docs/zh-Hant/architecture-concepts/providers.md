# 服務提供者

## 註冊服務提供者

`bootstrap/providers.rs` 列出 HTTP 與 Artisan 共用的應用程式服務提供者，並登記 `app/providers/app_service_provider.rs` 中的預設 `AppServiceProvider`。應用程式可在這個 Provider 的 `register` 和 `boot` 中接入服務。

選定的 Kernel 載入設定，執行所有 Provider 的 `register` 後，再依序執行 `boot`。詳見[請求生命週期](lifecycle.md)。

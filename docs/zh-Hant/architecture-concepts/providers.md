# 服務提供者

## 註冊服務提供者

`bootstrap/providers.rs` 列出 HTTP 與 Artisan 共用的應用程式服務提供者，並登記 `app/providers/app_service_provider.rs` 中的預設 `AppServiceProvider`。應用程式可在這個 Provider 的 `register` 和 `boot` 中接入服務。

選定的 Kernel 載入設定，執行所有 Provider 的 `register` 後，再依序執行 `boot`。詳見[請求生命週期](lifecycle.md)。

## 撰寫 Service Provider

實作 `openrust::support::ServiceProvider`，在 `bootstrap/providers.rs` 列出 Provider。覆寫 `register` 綁定服務，覆寫 `boot` 在所有 Provider 註冊後初始化服務。兩者預設均成功且不執行操作，只需實作所需鉤子。`boot` 回傳非同步 Future，初始化可以等待 I/O。

Provider 按具體 Rust 類型去重，保留首次註冊。成功引導具備冪等性。註冊或引導失敗後不再執行後續階段，同一個應用不能重試已經部分完成的引導。錯誤標明 Provider 和階段，並保留原始錯誤供報告使用；重試初始化需建立新應用。

Provider 鉤子在所屬應用的門面作用域內執行。HTTP 與 Artisan 使用同樣的 Provider 生命週期，不同應用的執行作用域和容器彼此隔離。參閱[容器綁定](container.md#綁定)和[門面](facades.md#門面的運作原理)。

# 目錄結構

## 應用程式目錄

根目錄的 `main.rs` 連接 Tokio 和 `bootstrap::createApplication`。`Cargo.toml` 與 `Cargo.lock` 宣告應用程式、工作區和鎖定的相依套件。應用程式程式碼位於 `app/`，編譯型設定載入器位於 `config/`，Web 和主控台路由宣告位於 `routes/`。

框架原始碼位於 `vendor/openrust/framework/`。檢視位於 `resources/views/`，網站檔案位於 `public/`，公開儲存磁碟位於 `storage/app/public/`。測試放在根應用程式或所屬 crate 的 `tests/` 目錄。

## Bootstrap 目錄

應用程式的 `bootstrap/` 目錄存放建立應用程式所需的裝配宣告。

`bootstrap/app.rs` 宣告設定、路由、中介軟體和命令入口。`bootstrap/mod.rs` 向二進位入口匯出 `bootstrap::createApplication`。`bootstrap/providers.rs` 列出應用程式的[服務提供者](../architecture-concepts/providers.md#註冊服務提供者)。

裝配入口透過 `withRouting` 宣告 Web 路由、控制台路由和健康路徑。Rust 使用 `Routing` Builder 表達這些選項，並透過 `withConfig`、`withProviders` 和 `withCommands` 註冊編譯進應用程式的設定、Provider 和命令。

框架使用這些宣告的順序見[請求生命週期](../architecture-concepts/lifecycle.md)。

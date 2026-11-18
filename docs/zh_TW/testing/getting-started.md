# 測試入門

## 執行測試 {#running-tests}

整合測試放在應用程式的 `tests/` 或相應 crate 下。使用已提交的鎖定檔執行工作區測試。

透過 Artisan 執行應用程式測試，或透過 Cargo 執行所有工作區成員的測試。使用 Artisan 時，可在測試篩選名稱前傳入 Cargo 選項，並在篩選名稱後以 `--` 傳入 Rust 測試執行器選項。遇到與 Artisan 同名的 Cargo 選項時，在 `test` 後立即加上一個 `--`，例如傳遞 Cargo 的 `--help`。

```sh
./artisan test --locked --test feature artisan_inspire_prints_an_attributed_quote -- --exact
cargo +1.94.0 test --workspace --locked
```

Artisan 從應用程式根目錄執行 Cargo，即時輸出結果並回傳 Cargo 的結束狀態。優先使用環境中的 `CARGO` 執行檔，未設定時從執行檔搜尋路徑呼叫 `cargo`。

## 測試環境 {#test-environment}

Artisan 給 Cargo 子程序設定 `APP_ENV=testing`。應用程式以該環境啟動時優先讀取 `.env.testing`，缺少時讀取 `.env`。程序環境和所選檔案都未定義 `CACHE_STORE` 或 `SESSION_DRIVER` 時，引導器分別補上 `array`。資料庫測試設定應放在測試環境中。修改環境設定前，可用 `./artisan config:clear` 清除應用程式的設定快取。

## 建立測試應用程式

用 `TestApp` 建立具有獨立暫存根目錄和服務容器的應用程式。啟動前可在 `test.root()` 下準備測試檔案。回呼接收一般應用程式 Builder，可以註冊本次測試所需的 Provider、路由和設定。

```rust
use openrust::{contracts::foundation::Application as _, testing::TestApp};

#[tokio::test]
async fn boots_a_test_application() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut test = TestApp::create(|builder| builder)?;
    std::fs::write(test.root().join("fixture.txt"), "test data")?;
    test.boot().await?;
    assert_eq!(test.app().config()?.get("app.env"), Some("testing".into()));
    Ok(())
}
```

`boot` 執行應用程式引導，無須繫結連接埠。預設設定使用 `testing` 環境，自訂設定載入器提供自己的設定。測試夾具離開作用域時清理其暫存目錄。測試輔助工具不修改程序環境和工作目錄。

在首次 `boot`、HTTP 請求或 Artisan 命令前，把 `.env.testing` 寫入 `test.root()`。設定載入器可以從環境參數讀取目前夾具內的值，收到的 `APP_ENV` 為 `testing`。夾具忽略從程序繼承的 `APP_CONFIG_CACHE`、`APP_PACKAGES_CACHE` 和 `APP_ROUTES_CACHE` 路徑。要為夾具指定自訂路由快取時，在它的 `.env.testing` 中設定 `APP_ROUTES_CACHE`。

## 建立測試 {#creating-tests}

Artisan 將功能測試寫入 `tests/feature/`，將單元測試寫入 `tests/unit/`。產生的功能測試會啟動暫存應用程式並檢查健康路由，單元測試使用 Rust 標準測試入口。

```sh
./artisan make:test HealthTest
./artisan make:test GreetingTest --unit
```

在 `tests/feature/main.rs` 宣告 `mod health_test;`，在 `tests/unit/main.rs` 宣告 `mod greeting_test;`，讓 Cargo 探索測試檔案。巢狀名稱也需要宣告各層模組。然後執行指定測試。

```sh
cargo +1.94.0 test --locked --test feature test_health_test
cargo +1.94.0 test --locked --test unit test_greeting_test
```

使用 `-f` 或 `--force` 覆寫既有測試檔案。在應用程式根目錄加入 `stubs/test.rs.stub` 或 `stubs/test.unit.rs.stub` 可自訂樣板。產生器會將 `{{test}}` 替換為 Rust 測試函式名稱。

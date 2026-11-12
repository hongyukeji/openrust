# 測試入門

## 執行測試

整合測試放在應用程式的 `tests/` 或相應 crate 下。使用已提交的鎖定檔執行工作區測試。

```sh
cargo +1.94.0 test --workspace --locked
```

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

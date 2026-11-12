# 測試替身

## 替換容器實例 {#mocking-objects}

在測試中實作應用程式服務的 trait，再使用 `TestApp::instance` 綁定替身。此方法啟動測試應用程式並替換其後續解析所用的綁定。在受測程式碼解析服務之前安裝替身。其他測試應用程式各自保留自己的綁定。

```rust
use std::sync::Arc;
use openrust::testing::TestApp;

#[tokio::test]
async fn replaces_a_service() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    app.instance(Arc::new(String::from("test value"))).await?;

    assert_eq!(app.app().make::<String>().await?.as_str(), "test value");
    Ok(())
}
```

事件斷言請見[事件測試](../digging-deeper/events.md#testing)。

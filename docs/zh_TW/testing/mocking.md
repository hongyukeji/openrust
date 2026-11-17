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

## 控制測試時間 {#interacting-with-time}

`TestApp::travelTo` 將應用程式時鐘凍結在指定的 `Date`。`freezeTime` 凍結目前時刻，`freezeSecond` 凍結在目前整秒。使用 `travel(5).minutes().await?` 向前移動，傳入負數可向後移動。天、週、月和年的計算使用測試應用程式的時區。呼叫 `travelBack().await?` 還原系統時間。

```rust
use openrust::{support::Date, testing::TestApp};

#[tokio::test]
async fn travels_five_minutes() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    app.freezeSecondFor(async |app, start| {
        let later = app.travel(5).minutes().await?;
        assert_eq!(later.asDateTime().timestamp(), start.asDateTime().timestamp() + 300);
        assert_eq!(Date::now(None)?, later);
        Ok(())
    }).await?;
    Ok(())
}
```

`travelToFor`、`freezeTimeFor` 和 `freezeSecondFor` 會在 Future 結束後還原原有時鐘，傳回錯誤或取消也一樣。測試應用程式呼叫 HTTP 和 Artisan 時，`Date::now`、`Date::today` 與 Eloquent 模型時間戳記共用該時鐘。其他測試應用程式、獨立程序、計時器以及外部資料庫和服務各自使用自己的時鐘。

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

## 門面呼叫記錄 {#facade-spies}

使用 `TestApp::spy` 在目前的測試應用程式中替換門面服務，並記錄後續呼叫。已有應用程式作用域時，也能直接呼叫門面的 `spy`。Rust 替身實作相同的服務 trait，並明確記錄測試需要檢查的呼叫。

```rust
use std::sync::Arc;
use openrust::{
    container::ServiceKey,
    support::facades::Facade,
    testing::{FacadeSpy, TestApp},
};

trait Reporter: Send + Sync {
    fn send(&self, report: &str);
}

struct Reports;

impl Facade for Reports {
    type Service = dyn Reporter;

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<dyn Reporter>()
    }
}

struct RecordingReporter(FacadeSpy<String>);

impl Reporter for RecordingReporter {
    fn send(&self, report: &str) {
        self.0.record("send", report.to_owned());
    }
}

#[tokio::test]
async fn records_reports() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    let spy = app.spy::<Reports, _>(|calls| Arc::new(RecordingReporter(calls))).await?;

    app.app().make::<dyn Reporter>().await?.send("daily");
    spy.shouldHaveReceived("send").with("daily".to_owned()).once();
    Ok(())
}
```

`received` 依呼叫順序傳回參數。可以用 `with` 依值篩選，或用 `withArgs` 依回呼篩選，再用 `once` 或 `times` 檢查次數。`shouldNotHaveReceived` 斷言方法未被呼叫。需要保留原服務行為時，替身可以把呼叫轉送給先前解析出的實例。

## 控制測試時間 {#interacting-with-time}

`TestApp::travelTo` 將應用程式時鐘凍結在指定的 `Date`。`freezeTime` 凍結目前時刻，`freezeSecond` 凍結在目前整秒。使用 `travel(5).minutes().await?` 向前移動，傳入負數可向後移動。天、週、月和年的計算使用測試應用程式的時區。呼叫 `travelBack().await?` 還原系統時間。

```rust
use openrust::{support::Date, testing::TestApp};

#[tokio::test]
async fn travels_five_minutes() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    app.freezeSecondFor(async |app, start| {
        let later = app.travel(5).minutes().await?;
        assert_eq!(later.asDateTime().timestamp().as_second(), start.asDateTime().timestamp().as_second() + 300);
        assert_eq!(Date::now(None)?, later);
        Ok(())
    }).await?;
    Ok(())
}
```

`travelToFor`、`freezeTimeFor` 和 `freezeSecondFor` 會在 Future 結束後還原原有時鐘，傳回錯誤或取消也一樣。測試應用程式呼叫 HTTP 和 Artisan 時，`Date::now`、`Date::today` 與 Eloquent 模型時間戳記共用該時鐘。其他測試應用程式、獨立程序、計時器以及外部資料庫和服務各自使用自己的時鐘。

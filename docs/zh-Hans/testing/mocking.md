# 测试替身

## 替换容器实例 {#mocking-objects}

在测试里实现应用服务的 trait，再用 `TestApp::instance` 绑定替身。该方法引导测试应用并替换其后续解析使用的绑定。在被测代码解析服务前安装替身。其他测试应用各自保留自己的绑定。

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

事件断言见[事件测试](../digging-deeper/events.md#testing)。

## 门面调用记录 {#facade-spies}

使用 `TestApp::spy` 在当前测试应用中替换门面服务并记录后续调用。已有应用作用域时，也可直接调用门面的 `spy`。Rust 替身实现相同的服务 trait，并显式记录测试需要检查的调用。

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
    spy.should_have_received("send").with("daily".to_owned()).once();
    Ok(())
}
```

`received` 按调用顺序返回参数。可用 `with` 按值筛选，或用 `with_args` 按回调筛选，再用 `once` 或 `times` 检查次数。`should_not_have_received` 断言方法未被调用。需要保留原服务的行为时，替身可以将调用转发给此前解析的实例。

## 控制测试时间 {#interacting-with-time}

`TestApp::travel_to` 将应用时钟冻结在给定的 `Date`。`freeze_time` 冻结当前时刻，`freeze_second` 冻结在当前整秒。用 `travel(5).minutes().await?` 向前移动，传负数可向后移动。天、周、月和年的计算使用测试应用的时区。调用 `travel_back().await?` 恢复系统时间。

```rust
use openrust::{support::Date, testing::TestApp};

#[tokio::test]
async fn travels_five_minutes() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut app = TestApp::create(|builder| builder)?;
    app.freeze_second_for(async |app, start| {
        let later = app.travel(5).minutes().await?;
        assert_eq!(later.as_date_time().timestamp().as_second(), start.as_date_time().timestamp().as_second() + 300);
        assert_eq!(Date::now(None)?, later);
        Ok(())
    }).await?;
    Ok(())
}
```

`travel_to_for`、`freeze_time_for` 和 `freeze_second_for` 会在 Future 结束后恢复之前的时钟，返回错误或取消也一样。测试应用调用 HTTP 和 Artisan 时，`Date::now`、`Date::today` 与 Eloquent 模型时间戳共用该时钟。其他测试应用、独立进程、计时器以及外部数据库和服务各自使用自己的时钟。

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

## 控制测试时间 {#interacting-with-time}

`TestApp::travelTo` 将应用时钟冻结在给定的 `Date`。`freezeTime` 冻结当前时刻，`freezeSecond` 冻结在当前整秒。用 `travel(5).minutes().await?` 向前移动，传负数可向后移动。天、周、月和年的计算使用测试应用的时区。调用 `travelBack().await?` 恢复系统时间。

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

`travelToFor`、`freezeTimeFor` 和 `freezeSecondFor` 会在 Future 结束后恢复之前的时钟，返回错误或取消也一样。测试应用调用 HTTP 和 Artisan 时，`Date::now`、`Date::today` 与 Eloquent 模型时间戳共用该时钟。其他测试应用、独立进程、计时器以及外部数据库和服务各自使用自己的时钟。

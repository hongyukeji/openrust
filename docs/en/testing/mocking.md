# Mocking

## Replacing a container instance {#mocking-objects}

Implement the application service trait in your test, then bind the substitute with `TestApp::instance`. This boots the fixture and replaces the binding for later resolutions in that application. Install the substitute before the code under test resolves the service. Other fixtures retain their own bindings.

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

For event-specific assertions, see [Testing events](../digging-deeper/events.md#testing).

## Interacting with time {#interacting-with-time}

`TestApp::travelTo` freezes the application clock at a `Date`. `freezeTime` uses the current instant, and `freezeSecond` drops its fractional second. Use `travel(5).minutes().await?` to move forward or pass a negative number to move backward. Days, weeks, months and years follow the fixture's application timezone. `travelBack().await?` restores the system clock.

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

`travelToFor`, `freezeTimeFor` and `freezeSecondFor` restore the previous clock when the future ends, including on error or cancellation. HTTP and Artisan invoked by the fixture use the same clock for `Date::now`, `Date::today` and Eloquent model timestamps. Other fixtures, independent processes, elapsed-time timers and external databases and services continue to use their own clocks.

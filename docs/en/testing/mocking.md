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

## Facade spies {#facade-spies}

Use `TestApp::spy` to replace a facade's service in one test application and record subsequent calls. Inside an active application scope, use the facade's `spy` method directly. Rust's replacement implements the same service trait and explicitly records the calls that the test should observe.

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

    fn getFacadeAccessor() -> ServiceKey {
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

`received` returns arguments in call order. Chain `with` or `withArgs` to filter by value or predicate, and use `once` or `times` to check counts. `shouldNotHaveReceived` checks that no call was recorded. A replacement may forward calls to a previously resolved service when the test needs its behavior.

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

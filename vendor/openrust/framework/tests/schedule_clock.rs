use std::{
    error::Error,
    time::{Duration, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use openrust::{
    console::{Application as Artisan, ConsoleError},
    foundation::configuration::ApplicationBuilder,
    support::{Date, DateTimezone, facades::Schedule},
    testing::TestApp,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn date(value: &str) -> Date {
    let time = DateTime::parse_from_rfc3339(value).unwrap();
    Date::fromUtc(time.with_timezone(&Utc), DateTimezone::UTC)
}

fn configure(builder: ApplicationBuilder) -> ApplicationBuilder {
    builder.withConfig(|_, _| {
        Ok([
            (
                "app".into(),
                serde_json::json!({"env": "testing", "schedule_timezone": "Asia/Tokyo"}),
            ),
            (
                "cache".into(),
                serde_json::json!({"default": "array", "stores": {"array": {"driver": "array"}}}),
            ),
        ]
        .into())
    })
}

fn daily(schedule: &mut openrust::console::scheduling::Schedule) -> Result<(), ConsoleError> {
    schedule
        .call(|_, output| {
            Box::pin(async move {
                output.stdout.push_str("clock task ran\n");
                Ok(())
            })
        })
        .name("Clock task")
        .dailyAt("09:30")?;
    Ok(())
}

fn register(_: &mut Artisan) -> Result<(), ConsoleError> {
    Schedule::configure(daily)
}

fn custom(_: &mut Artisan) -> Result<(), ConsoleError> {
    Schedule::configure(|schedule| {
        schedule.useClock(|| UNIX_EPOCH + Duration::from_secs(30 * 60));
        daily(schedule)
    })
}

fn repeating(_: &mut Artisan) -> Result<(), ConsoleError> {
    Schedule::configure(|schedule| {
        schedule
            .call(|_, output| {
                Box::pin(async move {
                    output.stdout.push_str("tick\n");
                    Ok(())
                })
            })
            .name("Clock repeat")
            .everySecond();
        schedule
            .call(|_, _| Box::pin(async { Ok(()) }))
            .name("Five-second repeat")
            .everyFiveSeconds();
        Ok(())
    })
}

#[tokio::test]
async fn schedule_commands_read_each_fixtures_clock_and_timezone() -> TestResult {
    let mut first = TestApp::create(|builder| configure(builder).withCommands(register))?;
    let mut second = TestApp::create(|builder| configure(builder).withCommands(register))?;
    first.travelTo(date("2024-01-01T00:30:04Z"));
    second.travelTo(date("1969-12-31T00:31:04Z"));

    let (left, right) = tokio::join!(
        first.artisan(["schedule:run"]),
        second.artisan(["schedule:run"])
    );
    left.assertSuccessful().expectsOutput("clock task ran");
    right.assertSuccessful();
    assert!(!right.stdout.contains("clock task ran"));
    let list = second
        .artisan(["schedule:list", "--timezone=Asia/Tokyo"])
        .await;
    list.assertSuccessful();
    assert!(
        list.stdout.contains("1970-01-01T09:30:00+09:00"),
        "{}",
        list.stdout
    );

    first
        .travelToFor(date("2030-01-02T00:31:04Z"), async |fixture, _| {
            let command = fixture.artisan(["schedule:run"]).await;
            command.assertSuccessful();
            assert!(!command.stdout.contains("clock task ran"));
            Ok(())
        })
        .await?;
    first
        .artisan(["schedule:run"])
        .await
        .assertSuccessful()
        .expectsOutput("clock task ran");
    Ok(())
}

#[tokio::test]
async fn explicit_scheduler_clock_takes_precedence_over_the_fixture_clock() -> TestResult {
    let mut app = TestApp::create(|builder| configure(builder).withCommands(custom))?;
    app.travelTo(date("2024-01-01T00:31:04Z"));
    app.artisan(["schedule:run"])
        .await
        .assertSuccessful()
        .expectsOutput("clock task ran");
    let list = app
        .artisan(["schedule:list", "--timezone=Asia/Tokyo"])
        .await;
    list.assertSuccessful();
    assert!(
        list.stdout.contains("1970-01-02T09:30:00+09:00"),
        "{}",
        list.stdout
    );
    Ok(())
}

#[tokio::test]
async fn sub_minute_tasks_keep_the_elapsed_time_deadline() -> TestResult {
    let mut app = TestApp::create(|builder| configure(builder).withCommands(repeating))?;
    for (at, task, next) in [
        (
            "2024-01-01T00:30:14.125Z",
            "Clock repeat",
            "2024-01-01T09:30:15+09:00",
        ),
        (
            "2024-01-01T00:30:55.000Z",
            "Five-second repeat",
            "2024-01-01T09:31:00+09:00",
        ),
        (
            "2024-01-01T00:30:59.900Z",
            "Clock repeat",
            "2024-01-01T09:31:00+09:00",
        ),
    ] {
        app.travelTo(date(at));
        let listing = app
            .artisan(["schedule:list", "--timezone=Asia/Tokyo"])
            .await;
        listing.assertSuccessful();
        assert!(
            listing
                .stdout
                .lines()
                .any(|line| line.contains(task) && line.ends_with(next)),
            "{}",
            listing.stdout
        );
    }
    app.travelTo(date("2024-01-01T00:30:59.900Z"));
    let result =
        tokio::time::timeout(Duration::from_secs(2), app.artisan(["schedule:run"])).await?;
    result.assertSuccessful().expectsOutput("tick");
    assert_eq!(
        result.stdout.lines().filter(|line| *line == "tick").count(),
        1
    );
    Ok(())
}

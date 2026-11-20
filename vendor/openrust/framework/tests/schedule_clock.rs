use std::{
    error::Error,
    time::{Duration, UNIX_EPOCH},
};

use jiff::Timestamp;
use openrust::{
    console::{Application as Artisan, ConsoleError},
    foundation::configuration::ApplicationBuilder,
    support::{Date, DateTimezone, facades::Schedule},
    testing::TestApp,
};
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn date(value: &str) -> Date {
    let time = value.parse::<Timestamp>().unwrap();
    Date::fromUtc(time, DateTimezone::UTC)
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
        .artisan(["schedule:list", "--timezone=Asia/Tokyo", "-v"])
        .await;
    list.assertSuccessful();
    assert!(
        list.stdout.contains("1970-01-01 09:30:00 +09:00"),
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
        .artisan(["schedule:list", "--timezone=Asia/Tokyo", "-v"])
        .await;
    list.assertSuccessful();
    assert!(
        list.stdout.contains("1970-01-02 09:30:00 +09:00"),
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
            "1969-12-31T23:59:59.900Z",
            "Clock repeat",
            "1970-01-01 09:00:00 +09:00",
        ),
        (
            "2024-01-01T00:30:14.125Z",
            "Clock repeat",
            "2024-01-01 09:30:15 +09:00",
        ),
        (
            "2024-01-01T00:30:55.000Z",
            "Five-second repeat",
            "2024-01-01 09:31:00 +09:00",
        ),
        (
            "2024-01-01T00:30:59.900Z",
            "Clock repeat",
            "2024-01-01 09:31:00 +09:00",
        ),
    ] {
        app.travelTo(date(at));
        let listing = app
            .artisan(["schedule:list", "--timezone=Asia/Tokyo", "-v"])
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

const CONVERSIONS: &[(&str, &str, &[&str])] = &[
    ("* * * * *", "Asia/Kathmandu", &["* * * * *"]),
    ("0 */2 * * *", "Asia/Tokyo", &["0 1-23/2 * * *"]),
    (
        "15,45 8 * * *",
        "Asia/Kolkata",
        &["45 2 * * *", "15 3 * * *"],
    ),
    ("0 8 * * 1-5", "Asia/Tokyo", &["0 23 * * 0-4"]),
    ("0 1 1 1 *", "Asia/Tokyo", &["0 16 31 12 *"]),
    ("0 1 1 3 *", "Asia/Tokyo", &["0 1 1 3 *"]),
    (
        "0 0 1 1-12/3 *",
        "Asia/Tokyo",
        &["0 15 31 3,12 *", "0 15 30 6,9 *"],
    ),
    ("0 8 * * MON-FRI", "Asia/Tokyo", &["0 8 * * MON-FRI"]),
    ("0 1 1 3 1", "Asia/Tokyo", &["0 1 1 3 1"]),
    ("0 8 L * *", "Asia/Tokyo", &["0 8 L * *"]),
    ("0 1 29 2 *", "Asia/Tokyo", &["0 1 29 2 *"]),
    (
        "0 1 * 1 *",
        "Asia/Tokyo",
        &["0 16 31 12 *", "0 16 1-30 1 *"],
    ),
    ("0 0 30 10 *", "Europe/London", &["0 0 30 10 *"]),
    ("0 0 30 6 *", "Europe/London", &["0 23 29 6 *"]),
];

fn list_json(_: &mut Artisan) -> Result<(), ConsoleError> {
    Schedule::configure(|schedule| {
        for (index, (cron, timezone, _)) in CONVERSIONS.iter().enumerate() {
            let event = schedule
                .command(format!("fixture:{index}"), [])
                .cron(cron)?
                .timezone(Date::timezone(timezone).unwrap())
                .environments(&["production"]);
            if index == 0 {
                event.onOneServer();
            }
        }
        schedule
            .call(|_, _| Box::pin(async { Ok(()) }))
            .everyFiveSeconds();
        Ok(())
    })
}

fn relative_list(_: &mut Artisan) -> Result<(), ConsoleError> {
    Schedule::configure(|schedule| {
        for (name, cron) in [
            ("fixture:february", "0 0 28 2 *"),
            ("fixture:march", "0 0 1 3 *"),
        ] {
            schedule
                .command(name, [])
                .cron(cron)?
                .timezone(jiff::tz::TimeZone::UTC);
        }
        Ok(())
    })
}

#[tokio::test]
async fn schedule_list_json_shifts_cron_and_keeps_laravel_metadata() -> TestResult {
    let mut app = TestApp::create(|builder| configure(builder).withCommands(list_json))?;
    app.travelTo(date("2023-10-29T00:00:00Z"));
    let reply = app
        .artisan([
            "schedule:list",
            "--json",
            "--next",
            "--environment=production",
        ])
        .await;
    reply.assertSuccessful();
    let entries: Vec<Value> = serde_json::from_str(&reply.stdout)?;
    let callback = &entries[0];
    let location = callback["command"].as_str().unwrap();
    assert!(location.starts_with("Closure at: ") && location.contains("schedule_clock.rs:"));
    assert_eq!(callback["next_due_date"], "2023-10-29 00:00:05 +00:00");
    assert_eq!(callback["next_due_date_human"], "5 seconds from now");
    assert_eq!(callback["repeat_seconds"], 5);
    assert_eq!(callback["on_one_server"], false);
    assert_eq!(callback["description"], Value::Null);
    assert_eq!(callback["environments"], json!([]));
    assert_eq!(callback["has_mutex"], false);
    for (index, (_, _, expressions)) in CONVERSIONS.iter().enumerate() {
        let command = format!("./artisan fixture:{index}");
        let matched: Vec<_> = entries
            .iter()
            .filter(|row| row["command"] == command)
            .collect();
        assert_eq!(matched.len(), expressions.len(), "{command}");
        for (row, expression) in matched.into_iter().zip(*expressions) {
            assert_eq!(row["expression"], *expression, "{command}");
            assert_eq!(row["timezone"], "UTC");
            assert_eq!(row["environments"], json!(["production"]));
            assert_eq!(row["repeat_seconds"], Value::Null);
            assert_eq!(row["on_one_server"], index == 0);
        }
    }
    let filtered = app
        .artisan(["schedule:list", "--json", "--environment=local"])
        .await;
    filtered.assertSuccessful();
    let entries: Vec<Value> = serde_json::from_str(&filtered.stdout)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["command"], callback["command"]);

    let empty = TestApp::create(configure)?
        .artisan(["schedule:list", "--json"])
        .await;
    empty.assertSuccessful();
    assert_eq!(empty.stdout, "[]\n");
    Ok(())
}

#[tokio::test]
async fn schedule_list_json_uses_complete_calendar_units_for_relative_dates() -> TestResult {
    let mut app = TestApp::create(|builder| configure(builder).withCommands(relative_list))?;
    app.travelTo(date("2023-01-31T00:00:00Z"));
    let reply = app.artisan(["schedule:list", "--json"]).await;
    reply.assertSuccessful();
    let entries: Vec<Value> = serde_json::from_str(&reply.stdout)?;
    assert_eq!(entries[0]["next_due_date_human"], "4 weeks from now");
    assert_eq!(entries[1]["next_due_date_human"], "1 month from now");
    Ok(())
}

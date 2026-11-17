use std::{error::Error, time::Duration};

use chrono::{DateTime, Timelike as _, Utc};
use openrust::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    database::{
        Value,
        eloquent::{Attributes, Model, ModelMetadata, ModelState},
    },
    foundation::{Application, configuration::Routing},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::Date,
    support::facades::{DB, Route},
    testing::TestApp,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn instant(text: &str) -> Date {
    Date::fromUtc(
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc),
        chrono_tz::UTC,
    )
}

fn date_text() -> Result<String, openrust::support::DateError> {
    Ok(Date::now(None)?.asDateTime().to_rfc3339())
}

fn clock_route(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let now = date_text().map_err(internal)?;
        let midnight = Date::today(None).map_err(internal)?;
        Response::json(StatusCode::OK, (now, midnight.asDateTime().to_rfc3339()))
    })
}

fn internal(source: impl Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(source),
    }
}

struct ClockCommand(CommandDefinition);

impl Command for ClockCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let now = date_text().map_err(|error| ConsoleError::new(error.to_string()))?;
            output.stdout.push_str(&format!("{now}\n"));
            Ok(0)
        })
    }
}

fn commands(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ClockCommand(CommandDefinition::new(
        clap::Command::new("clock:probe"),
    ))))
}

fn app() -> TestResult<TestApp> {
    TestApp::sqlite(|builder| {
        builder
            .withConfig(|_, _| {
                Ok([(
                    "app".into(),
                    serde_json::json!({"env":"testing", "timezone":"America/New_York"}),
                )]
                .into())
            })
            .withCommands(commands)
            .withRouting(Routing::default().web(|| {
                Route::get("/clock", clock_route)?;
                Route::get("/stamp", stamp_route)
            }))
    })
}

#[tokio::test]
async fn fixture_time_is_visible_to_http_artisan_and_model_timestamps() -> TestResult {
    let mut first = app()?;
    let mut second = app()?;
    let before = instant("2024-03-09T17:00:00.123456Z");
    first.travelTo(before);
    second.travelTo(instant("2024-11-19T01:00:00Z"));
    let (left, right) = tokio::join!(first.get("/clock"), second.get("/clock"));
    left?.assertOk().assertExactJson(&serde_json::json!([
        "2024-03-09T12:00:00.123456-05:00",
        "2024-03-09T00:00:00-05:00"
    ]));
    right?.assertOk().assertExactJson(&serde_json::json!([
        "2024-11-18T20:00:00-05:00",
        "2024-11-18T00:00:00-05:00"
    ]));

    let after = first.travel(1).day().await?;
    assert_eq!(
        after.asDateTime().with_timezone(&Utc),
        instant("2024-03-10T16:00:00.123456Z")
            .asDateTime()
            .with_timezone(&Utc)
    );
    assert_eq!(first.freezeTime().await?, after);
    assert_eq!(first.freezeSecond().await?.asDateTime().nanosecond(), 0);
    first
        .artisan(["clock:probe"])
        .await
        .assertSuccessful()
        .expectsOutput("2024-03-10T12:00:00-04:00");
    first.database().await?.statement("CREATE TABLE stamps (id INTEGER PRIMARY KEY, label TEXT, created_at DATETIME, updated_at DATETIME)", &[]).await?;
    first
        .get("/stamp")
        .await?
        .assertOk()
        .assertExactJson(&serde_json::json!([
            "2024-03-10 12:00:00",
            "2024-03-10 12:00:00"
        ]));

    first.travel(-1).minute().await?;
    let connection = first.database().await?;
    let model = Stamp::queryOn(&connection)
        .create(Attributes::from([("label".into(), "direct".into())]))
        .await?;
    assert_eq!(
        model.getRawOriginal("created_at"),
        Some(&Value::from("2024-03-10 11:59:00"))
    );
    assert!(first.travel(i64::MAX).milliseconds().await.is_err());
    let wall_clock = Utc::now();
    let restored = first.travelBack().await?.asDateTime().with_timezone(&Utc);
    assert!((wall_clock..=Utc::now()).contains(&restored));
    Ok(())
}

#[tokio::test]
async fn scoped_time_restores_after_nested_errors_and_cancellation() -> TestResult {
    let mut fixture = app()?;
    let outer = instant("2024-01-31T17:00:00.525Z");
    fixture.travelTo(outer);
    let value = fixture
        .freezeSecondFor(async |fixture, time| {
            assert_eq!(Date::now(None)?, time);
            let shifted = fixture.travel(1).month().await?;
            assert_eq!(
                shifted.asDateTime().to_rfc3339(),
                "2024-03-02T12:00:00-05:00"
            );
            let rejected = fixture
                .travelToFor(outer, async |_, _| -> TestResult {
                    Err(std::io::Error::other("scope failed").into())
                })
                .await;
            assert!(rejected.is_err());
            assert_eq!(Date::now(None)?, shifted);
            Ok(42)
        })
        .await?;
    assert_eq!(value, 42);
    assert_eq!(
        fixture
            .freezeTimeFor(async |_, time| Ok(time))
            .await?
            .asDateTime()
            .with_timezone(&Utc),
        outer.asDateTime().with_timezone(&Utc)
    );

    let cancelled = tokio::time::timeout(
        Duration::ZERO,
        fixture.travelToFor(instant("2030-01-01T12:00:00Z"), async |_, _| {
            std::future::pending::<TestResult>().await
        }),
    )
    .await;
    assert!(cancelled.is_err());
    fixture
        .artisan(["clock:probe"])
        .await
        .assertSuccessful()
        .expectsOutput("2024-01-31T12:00:00.525-05:00");
    Ok(())
}

struct Stamp(ModelState);

impl Model for Stamp {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("stamps").fillable(&["label"]);
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

fn stamp_route(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let database = DB::connection(None).map_err(internal)?;
        let row = Stamp::queryOn(&database)
            .create(Attributes::from([("label".into(), "test".into())]))
            .await
            .map_err(internal)?;
        let timestamps = ["created_at", "updated_at"].map(|name| match row.getRawOriginal(name) {
            Some(Value::Text(value)) => Some(value.as_str()),
            _ => None,
        });
        Response::json(StatusCode::OK, timestamps)
    })
}

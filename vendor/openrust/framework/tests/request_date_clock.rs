use std::error::Error;

use jiff::Timestamp;
use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{Date, DateTimezone, facades::Route},
    testing::TestApp,
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn endpoint(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let format = request.query(Some("format"))?;
        let timezone = request.query(Some("timezone"))?;
        let date = request
            .date(
                "when",
                format.as_ref().and_then(serde_json::Value::as_str),
                timezone.as_ref().and_then(serde_json::Value::as_str),
            )
            .await?;
        let formatted = date
            .map(|date| date.format("Y-m-d H:i:s.u P e"))
            .transpose()
            .map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
        Response::json(StatusCode::OK, formatted)
    })
}

fn fixture() -> TestResult<TestApp> {
    TestApp::create(|builder| {
        builder
            .with_config(|_, _| {
                Ok([(
                    "app".into(),
                    serde_json::json!({"env": "testing", "timezone": "Asia/Tokyo"}),
                )]
                .into())
            })
            .with_routing(Routing::default().web(|| Route::get("/date", endpoint)))
    })
}

fn date(text: &str) -> Date {
    Date::from_utc(text.parse::<Timestamp>().unwrap(), DateTimezone::UTC)
}

fn uri(input: &str, format: Option<&str>, timezone: Option<&str>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("when", input);
    if let Some(format) = format {
        query.append_pair("format", format);
    }
    if let Some(timezone) = timezone {
        query.append_pair("timezone", timezone);
    }
    format!("/date?{}", query.finish())
}

#[tokio::test]
async fn request_date_matches_carbon_with_a_frozen_application_clock() -> TestResult {
    let mut app = fixture()?;
    app.travel_to(date("2024-12-31T23:41:22.123456Z"));

    // Fixed InteractsWithData and Carbon 3.8.4 on PHP 8.5, Asia/Tokyo.
    for (input, format, expected) in [
        ("now", None, "2025-01-01 08:41:22.123456 +09:00 Asia/Tokyo"),
        (
            "tomorrow",
            None,
            "2025-01-02 00:00:00.000000 +09:00 Asia/Tokyo",
        ),
        (
            "+2 hours",
            None,
            "2025-01-01 10:41:22.123456 +09:00 Asia/Tokyo",
        ),
        (
            "January 5",
            None,
            "2025-01-05 08:41:22.123456 +09:00 Asia/Tokyo",
        ),
        (
            "February",
            None,
            "2025-02-01 08:41:22.123456 +09:00 Asia/Tokyo",
        ),
        (
            "08:30",
            None,
            "2025-01-01 08:30:00.000000 +09:00 Asia/Tokyo",
        ),
        ("0830", None, "2025-01-01 08:30:00.123456 +09:00 Asia/Tokyo"),
        (
            "08:30",
            Some("H:i"),
            "2025-01-01 08:30:00.000000 +09:00 Asia/Tokyo",
        ),
        (
            "20",
            Some("d"),
            "2025-01-20 08:41:22.123456 +09:00 Asia/Tokyo",
        ),
        (
            "20",
            Some("!d"),
            "1970-01-20 00:00:00.000000 +09:00 Asia/Tokyo",
        ),
        (
            "tomorrow America/New_York",
            None,
            "2025-01-02 00:00:00.000000 +09:00 Asia/Tokyo",
        ),
        (
            "January 5 08:30 Europe/Paris",
            None,
            "2025-01-05 08:30:00.000000 +09:00 Asia/Tokyo",
        ),
        (
            "2026-01-02",
            None,
            "2026-01-02 00:00:00.000000 +09:00 Asia/Tokyo",
        ),
        (
            "2026-01-02 08:00 Europe/Paris",
            None,
            "2026-01-02 08:00:00.000000 +01:00 Europe/Paris",
        ),
        (
            "2026-02-04 +2 hours America/New_York",
            None,
            "2026-02-04 02:00:00.000000 -05:00 America/New_York",
        ),
        (
            "20 Europe/Paris",
            Some("d e"),
            "2025-01-20 08:41:22.123456 +01:00 Europe/Paris",
        ),
        ("@0", None, "1970-01-01 00:00:00.000000 +00:00 +00:00"),
    ] {
        let response = app.get(&uri(input, format, None)).await?;
        response.assert_ok();
        assert_eq!(
            response.json::<String>()?,
            expected,
            "{input:?}, {format:?}"
        );
    }
    app.get(&uri(" ", None, None))
        .await?
        .assert_ok()
        .assert_exact_json(&serde_json::Value::Null);
    app.get(&uri("not a date", None, None))
        .await?
        .assert_status(StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn relative_request_dates_restore_the_previous_clock_and_follow_explicit_timezone()
-> TestResult {
    let mut first = fixture()?;
    let mut second = fixture()?;
    first.travel_to(date("2024-12-31T23:41:22.123456Z"));
    second.travel_to(date("2020-02-28T23:00:00Z"));
    let request = uri("tomorrow", None, Some("UTC"));
    let (left, right) = tokio::join!(first.get(&request), second.get(&request));
    left?
        .assert_ok()
        .assert_exact_json(&serde_json::json!("2025-01-01 00:00:00.000000 +00:00 UTC"));
    right?
        .assert_ok()
        .assert_exact_json(&serde_json::json!("2020-02-29 00:00:00.000000 +00:00 UTC"));
    first
        .travel_to_for(date("2024-03-10T06:30:00Z"), async |app, _| {
            let request = uri("tomorrow", None, Some("America/New_York"));
            app.get(&request)
                .await?
                .assert_ok()
                .assert_exact_json(&serde_json::json!(
                    "2024-03-11 00:00:00.000000 -04:00 America/New_York"
                ));
            Ok(())
        })
        .await?;
    first
        .get(&request)
        .await?
        .assert_ok()
        .assert_exact_json(&serde_json::json!("2025-01-01 00:00:00.000000 +00:00 UTC"));
    Ok(())
}

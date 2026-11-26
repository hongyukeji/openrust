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
        let instance = request
            .date(
                "when",
                format.as_ref().and_then(serde_json::Value::as_str),
                timezone.as_ref().and_then(serde_json::Value::as_str),
            )
            .await;
        // 将独占请求按正式借出契约交给容器，再从公开 Request 门面读取同一数据。
        let facade = request
            .with_container(openrust::support::facades::Request::date(
                "when",
                format.as_ref().and_then(serde_json::Value::as_str),
                timezone.as_ref().and_then(serde_json::Value::as_str),
            ))
            .await;
        let date = match (instance, facade) {
            (Ok(date), Ok(facade)) => {
                assert_eq!(
                    date.as_ref()
                        .map(|value| value.format("Y-m-d H:i:s.u P e").unwrap()),
                    facade
                        .as_ref()
                        .map(|value| value.format("Y-m-d H:i:s.u P e").unwrap())
                );
                date
            }
            (Err(error), Err(_)) => return Err(error),
            _ => panic!("请求实例与门面日期结果不一致"),
        };
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
            .with_routing(Routing::default().web(|| {
                Route::get("/date", endpoint)?;
                Ok(())
            }))
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

    // 格式分支按固定原类与Carbon3.10.3实跑；冻结值是UTC对象，应用为Asia/Tokyo。
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
            "2024-12-31 08:30:00.000000 +00:00 UTC",
        ),
        ("20", Some("d"), "2024-12-20 23:41:22.123456 +00:00 UTC"),
        ("20", Some("!d"), "1970-01-20 00:00:00.000000 +00:00 UTC"),
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
            "2024-12-20 23:41:22.123456 +01:00 Europe/Paris",
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

#[tokio::test]
async fn actual_request_date_instance_and_facade_consume_factory_strictness_and_mock_timezone()
-> TestResult {
    use openrust::support::facades::{App, Date as DateFacade, Facade};
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/date/request-format-creation-contract.json"
    ))?;
    assert_eq!(cases.len(), 30);
    let mut app = fixture()?;
    app.travel_to(date("2024-12-31T23:41:22.123456Z"));
    app.boot().await?;
    for case in cases {
        App::set_facade_application(Some(app.app()), async {
            DateFacade::use_strict_mode(case["strict"].as_bool().unwrap())
        })
        .await?;
        let input = case["input"]
            .as_str()
            .map_or_else(|| case["input"].to_string(), str::to_owned);
        let response = app
            .get(&uri(
                &input,
                case["format"].as_str(),
                case["timezone"].as_str(),
            ))
            .await?;
        if case["result"].get("error").is_some() {
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "{case}: {}",
                String::from_utf8_lossy(response.body())
            );
        } else {
            response.assert_ok().assert_exact_json(&case["result"]);
        }
    }
    Ok(())
}

use openrust::{
    support::{
        Date as DateValue, DateError,
        facades::{App, Date, Facade},
    },
    testing::TestApp,
};
use serde_json::{Value, json};
async fn app() -> TestApp {
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"UTC"}))].into()))
    })
    .unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn actual_original_format_creators_cover_mock_timezone_resets_strictness_and_nul_errors() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/date/format-creation-contract.json")).unwrap();
    assert_eq!(cases.len(), 276);
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        let mock = Date::parse(
            "2024-03-10 08:09:10.123456",
            Some(DateValue::timezone("America/New_York").unwrap()),
        )
        .unwrap();
        Date::set_test_now(Some(mock)).unwrap();
        let factory = Date::get_facade_root().unwrap();
        assert!(Date::is_strict_mode_enabled().unwrap());
        for case in cases {
            Date::use_strict_mode(case["strict"].as_bool().unwrap()).unwrap();
            assert_eq!(factory.is_strict_mode_enabled(), case["strict"]);
            let zone = case["timezone"]
                .as_str()
                .map(|zone| DateValue::timezone(zone).unwrap());
            let format = case["format"].as_str().unwrap();
            let input = case["input"].as_str().unwrap();
            let result = if case["method"] == "createFromFormat" {
                Date::create_from_format(format, input, zone.clone())
            } else {
                Date::raw_create_from_format(format, input, zone.clone())
            };
            let direct = if case["method"] == "createFromFormat" {
                factory.create_from_format(format, input, zone)
            } else {
                factory.raw_create_from_format(format, input, zone)
            };
            if case["result"].get("error").is_some() {
                assert_eq!(result, Err(DateError::InvalidExpression), "{case}");
                assert!(direct.is_err());
            } else if case["result"].is_null() {
                assert_eq!(result, Ok(None), "{case}");
                assert_eq!(direct, Ok(None));
            } else {
                let date = result
                    .unwrap_or_else(|e| panic!("{case}: {e}"))
                    .unwrap_or_else(|| panic!("{case}: None"));
                assert_eq!(
                    date.format("Y-m-d H:i:s.uP").unwrap(),
                    case["result"]["date"],
                    "{case}"
                );
                assert_eq!(date.timezone_name(), case["result"]["zone"], "{case}");
                assert_eq!(
                    direct.unwrap().unwrap().format("Y-m-d H:i:s.uP").unwrap(),
                    case["result"]["date"]
                );
            }
        }
        Date::use_strict_mode(None).unwrap();
        assert!(Date::is_strict_mode_enabled().unwrap());
    })
    .await;
}
#[tokio::test]
async fn application_and_standalone_factories_own_strict_mode_and_accept_numeric_units() {
    let a = app().await;
    let b = app().await;
    App::set_facade_application(Some(a.app()), async {
        Date::use_strict_mode(false).unwrap();
        assert_eq!(
            Date::create_from_format("Y-m-d", "wrong", None).unwrap(),
            None
        );
        App::set_facade_application(Some(b.app()), async {
            assert!(Date::is_strict_mode_enabled().unwrap());
            assert!(Date::create_from_format("Y-m-d", "wrong", None).is_err());
        })
        .await;
        assert!(!Date::is_strict_mode_enabled().unwrap());
        let date = Date::create_from_format("!Y", 2024, None).unwrap().unwrap();
        assert_eq!(date.format("Y-m-d H:i:s").unwrap(), "2024-01-01 00:00:00");
        assert_eq!(
            Date::raw_create_from_format("U", 0, None)
                .unwrap()
                .unwrap()
                .format("U")
                .unwrap(),
            "0"
        );
        assert_eq!(
            Date::create_from_format("Y", json!(true), None),
            Err(DateError::InvalidExpression)
        );
    })
    .await;
    let one = openrust::support::DateFactory::default();
    let two = openrust::support::DateFactory::default();
    one.use_strict_mode(false);
    assert!(!one.is_strict_mode_enabled());
    assert!(two.is_strict_mode_enabled());
}

struct FormatJob(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
impl openrust::contracts::queue::JobHandler for FormatJob {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        _: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async move {
            let result = (|| -> Result<String, DateError> {
                assert!(!Date::is_strict_mode_enabled()?);
                assert!(Date::create_from_format("Y-m-d", "wrong", None)?.is_none());
                Date::create_from_format("Y-m-d", "2024-01-02", None)?
                    .unwrap()
                    .format("Y-m-d H:i:s.uP e")
            })()
            .map_err(openrust::queue::QueueError::execution)?;
            self.0.lock().unwrap().push(result);
            Ok(())
        })
    }
}
#[tokio::test]
async fn actual_artisan_worker_consumes_formatted_dates_with_its_application_clock_and_strict_mode()
{
    use openrust::{
        queue::{DispatchOptions, Payload},
        support::facades::Queue,
    };
    let mut app=TestApp::sqlite(|b|b.with_config(|_,_|Ok([
        ("app".into(),json!({"env":"testing","timezone":"Asia/Tokyo"})),
        ("queue".into(),json!({"default":"database","connections":{"database":{"driver":"database","table":"date_jobs","retry_after":30}},"failed":{"driver":"null"}})),
    ].into()))).unwrap();
    app.travel_to(DateValue::from_utc(
        "2024-12-31T23:41:22.123456Z".parse().unwrap(),
        DateValue::timezone("America/New_York").unwrap(),
    ));
    app.boot().await.unwrap();
    let db = app.database().await.unwrap();
    db.get_schema_builder()
        .create("date_jobs", |t| {
            t.id();
            t.string("queue", 255).index();
            t.long_text("payload");
            t.unsigned_integer("attempts");
            t.unsigned_big_integer("reserved_at").nullable();
            t.unsigned_big_integer("available_at");
            t.unsigned_big_integer("created_at");
        })
        .await
        .unwrap();
    let completed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    App::set_facade_application(Some(app.app()), async {
        Date::use_strict_mode(false).unwrap();
        Queue::register(
            "date-format",
            std::sync::Arc::new(FormatJob(completed.clone())),
        )
        .unwrap();
        Queue::without_interruption_polling().unwrap();
        for _ in 0..2 {
            Queue::dispatch(
                Payload::new("date-format", ()).unwrap(),
                DispatchOptions {
                    queue: Some("date-isolated".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        }
    })
    .await;
    app.artisan([
        "queue:work",
        "database",
        "--queue=date-isolated",
        "--stop-when-empty",
        "--sleep=0",
        "--timeout=0",
        "--tries=1",
    ])
    .await
    .assert_successful();
    assert_eq!(
        *completed.lock().unwrap(),
        vec!["2024-01-02 18:41:22.123456-05:00 America/New_York"; 2]
    );
    App::set_facade_application(Some(app.app()), async {
        assert_eq!(
            Queue::connection(None)
                .unwrap()
                .size(Some("date-isolated"))
                .await
                .unwrap(),
            0
        );
    })
    .await;
}

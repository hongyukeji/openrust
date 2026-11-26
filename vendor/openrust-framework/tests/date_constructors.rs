use openrust::{
    support::{
        DateComponents, DateError, DateFactory, StrictDateComponents,
        facades::{App, Date, Facade},
    },
    testing::TestApp,
};
use serde_json::{Value, json};
fn parts(value: &Value) -> DateComponents {
    let a = value.as_array().unwrap();
    DateComponents {
        year: a[0].as_i64(),
        month: a[1].as_i64(),
        day: a[2].as_i64(),
        hour: a[3].as_i64(),
        minute: a[4].as_i64(),
        second: a[5].as_f64(),
    }
}
fn strict_parts(value: &Value) -> StrictDateComponents {
    let parts = parts(value);
    // 原PHP调用方的弱类型小数秒会产生弃用提示并截断；Rust调用方须显式给整数。
    StrictDateComponents {
        year: parts.year,
        month: parts.month,
        day: parts.day,
        hour: parts.hour,
        minute: parts.minute,
        second: parts
            .second
            .map(|second| format!("{:.0}", second.trunc()).parse().unwrap()),
    }
}
fn assert_result(result: Result<Option<openrust::support::Date>, DateError>, row: &Value) {
    let expected = &row["result"];
    if expected.get("error").is_some() {
        assert!(result.is_err(), "{row}");
    } else if expected.is_null() {
        assert_eq!(result.unwrap(), None, "{row}");
    } else if row
        .get("parts")
        .is_some_and(|parts| parts[0].as_i64() == Some(10000))
    {
        assert_eq!(result, Err(DateError::OutOfRange));
    } else {
        let actual = result.unwrap().unwrap();
        assert_eq!(
            actual.format("Y-m-d H:i:s.uP").unwrap(),
            expected["date"],
            "{row}"
        );
        assert_eq!(actual.timezone_name(), expected["zone"], "{row}");
    }
}
#[tokio::test]
async fn six_constructor_entries_match_fixed_original_components_null_defaults_overflow_dst_and_strict_modes()
 {
    let rows: Value =
        serde_json::from_str(include_str!("fixtures/date/constructors-contract.json")).unwrap();
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"Asia/Tokyo"}))].into()))
    })
    .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Date::set_test_now(Some(
            Date::parse(
                "2024-03-10 01:59:59.123456",
                Some(openrust::support::Date::timezone("America/New_York").unwrap()),
            )
            .unwrap(),
        ))
        .unwrap();
        let factory = Date::get_facade_root().unwrap();
        for row in rows.as_array().unwrap() {
            let zone = row["timezone"]
                .as_str()
                .map(|zone| openrust::support::Date::timezone(zone).unwrap());
            let method = row["method"].as_str().unwrap();
            Date::use_strict_mode(row["strict"].as_bool().unwrap_or(true)).unwrap();
            let (actual, direct) = match method {
                "create" => (
                    Date::create(parts(&row["parts"]), zone.clone()),
                    factory.create(parts(&row["parts"]), zone),
                ),
                "createSafe" => (
                    Date::create_safe(parts(&row["parts"]), zone.clone()),
                    factory.create_safe(parts(&row["parts"]), zone),
                ),
                "createStrict" => (
                    Date::create_strict(strict_parts(&row["parts"]), zone.clone()).map(Some),
                    factory
                        .create_strict(strict_parts(&row["parts"]), zone)
                        .map(Some),
                ),
                "createFromDate" => (
                    Date::create_from_date(None, Some(1), Some(1), zone.clone()),
                    factory.create_from_date(None, Some(1), Some(1), zone),
                ),
                "createMidnightDate" => (
                    Date::create_midnight_date(None, Some(1), Some(1), zone.clone()),
                    factory.create_midnight_date(None, Some(1), Some(1), zone),
                ),
                "createFromTime" => (
                    Date::create_from_time(None, Some(12), None, zone.clone()).map(Some),
                    factory
                        .create_from_time(None, Some(12), None, zone)
                        .map(Some),
                ),
                _ => panic!("{row}"),
            };
            assert_result(actual, row);
            assert_result(direct, row);
            assert_eq!(
                Date::is_strict_mode_enabled().unwrap(),
                row["strict"].as_bool().unwrap_or(true),
                "严格创建不能泄漏模式变化"
            );
        }
    })
    .await;
}
#[test]
fn component_defaults_are_explicit_and_invalid_native_ranges_do_not_silently_wrap() {
    let factory = DateFactory::default();
    assert_eq!(
        factory
            .create(DateComponents::default(), None)
            .unwrap()
            .unwrap()
            .format("Y-m-d H:i:s")
            .unwrap(),
        "0000-01-01 00:00:00"
    );
    assert_eq!(
        factory.create(
            DateComponents {
                second: Some(f64::NAN),
                ..Default::default()
            },
            None
        ),
        Err(DateError::OutOfRange)
    );
    assert_eq!(
        factory.create(
            DateComponents {
                year: Some(i64::MIN),
                ..Default::default()
            },
            None
        ),
        Err(DateError::OutOfRange)
    );
}

fn constructed_endpoint(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let parts = DateComponents {
            year: Some(2024),
            month: Some(2),
            day: Some(30),
            ..Default::default()
        };
        let result=(||->Result<Value,DateError>{
   Ok(json!({"time_text":Date::create_from_time_string("12:34 UTC",None)?.format("H:i:s")?,"normalized":Date::create(parts,None)?.unwrap().format("Y-m-d")?,"safe_failed":Date::create_safe(parts,None).is_err(),"midnight":Date::create_midnight_date(None,None,None,None)?.unwrap().format("Y-m-d H:i:s")?}))
  })().map_err(|source|openrust::http::HttpError::Internal{source:Box::new(source)})?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, result)
    })
}
#[tokio::test]
async fn real_http_consumer_uses_same_clock_and_returns_safe_failure_without_changing_strict_mode()
{
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"UTC"}))].into()))
            .with_routing(
                openrust::foundation::configuration::Routing::default().web(|| {
                    openrust::support::facades::Route::get("/created", constructed_endpoint)
                }),
            )
    })
    .unwrap();
    app.travel_to(
        DateFactory::default()
            .parse("2024-03-10 01:59:59.123456", None)
            .unwrap(),
    );
    app.get("/created")
        .await
        .unwrap()
        .assert_ok()
        .assert_exact_json(
            &json!({"time_text":"12:34:00","normalized":"2024-03-01","safe_failed":true,"midnight":"2024-03-10 00:00:00"}),
        );
    App::set_facade_application(Some(app.app()), async {
        assert!(Date::is_strict_mode_enabled().unwrap());
    })
    .await;
}

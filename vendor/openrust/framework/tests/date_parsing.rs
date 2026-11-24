use openrust::{
    support::{
        Date as DateValue, DateError, DateInput, DateTimezone,
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
async fn original_php_parse_vectors_match_public_facade_values_formats_and_zone_identities() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/date/parsing-contract.json")).unwrap();
    assert_eq!(cases.len(), 23);
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        let now = DateValue::from_utc(
            "2024-03-10T12:00:00.123456Z".parse().unwrap(),
            DateTimezone::UTC,
        );
        Date::with_test_now(Some(now), async {
            for case in cases {
                let zone = DateValue::timezone(case["timezone"].as_str().unwrap()).unwrap();
                let result = Date::parse(case["input"].clone(), Some(zone.clone()));
                let raw = Date::raw_parse(case["input"].clone(), Some(zone));
                if case["result"].get("error").is_some() {
                    assert!(
                        matches!(result, Err(DateError::InvalidExpression)),
                        "{case}: {result:?}"
                    );
                    assert!(raw.is_err());
                    continue;
                }
                let date = result.unwrap_or_else(|error| panic!("{case}: {error}"));
                assert_eq!(
                    date.format("Y-m-d H:i:s.uP").unwrap(),
                    case["result"]["date"],
                    "{case}"
                );
                assert_eq!(date.timezone_name(), case["result"]["zone"], "{case}");
                assert_eq!(
                    raw.unwrap().format("Y-m-d H:i:s.uP").unwrap(),
                    case["result"]["date"]
                );
                assert_eq!(
                    Date::instance(&date).unwrap().format("e T").unwrap(),
                    date.format("e T").unwrap()
                );
            }
        })
        .await
        .unwrap();
    })
    .await;
}
#[tokio::test]
async fn native_inputs_keep_existing_dates_and_reject_non_finite_or_wrong_json_types() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        for input in [json!(true), json!(false), json!([]), json!({})] {
            assert_eq!(Date::parse(input, None), Err(DateError::InvalidExpression));
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(Date::parse(value, None), Err(DateError::OutOfRange));
        }
        let original = Date::parse("2024-01-01 03:04:05 CST", None).unwrap();
        assert_eq!(
            Date::parse(&original, None)
                .unwrap()
                .format("Y-m-d H:i:s e T")
                .unwrap(),
            "2024-01-01 03:04:05 CST CST"
        );
        let converted = Date::parse(
            original,
            Some(DateValue::timezone("Asia/Shanghai").unwrap()),
        )
        .unwrap();
        assert_eq!(
            converted.format("Y-m-d H:i:s e").unwrap(),
            "2024-01-01 17:04:05 Asia/Shanghai"
        );
        assert_eq!(
            Date::parse(0, None).unwrap().format("U.u").unwrap(),
            "0.000000"
        );
        assert_eq!(
            Date::parse(-1i64, None).unwrap().format("U.u").unwrap(),
            "-1.000000"
        );
        assert_eq!(
            Date::parse(1.25, None).unwrap().format("U.u").unwrap(),
            "1.250000"
        );
        assert_eq!(Date::parse(u64::MAX, None), Err(DateError::OutOfRange));
        Date::with_test_now(Some(converted.clone()), async {
            assert_eq!(
                Date::parse(DateInput::Now, None)
                    .unwrap()
                    .as_date_time()
                    .timestamp(),
                converted.as_date_time().timestamp()
            );
            assert_eq!(
                Date::parse(Value::Null, None)
                    .unwrap()
                    .as_date_time()
                    .timestamp(),
                converted.as_date_time().timestamp()
            );
        })
        .await
        .unwrap();
    })
    .await;
}
#[tokio::test]
async fn parsed_zone_identity_survives_civil_arithmetic_and_uses_the_new_instant_when_formatted() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        let date = Date::parse("2024-01-01 03:04:05 CST", None).unwrap();
        let next = date
            .plus(
                openrust::support::days(1.0).unwrap(),
                openrust::support::MonthOverflow::Carry,
            )
            .unwrap();
        assert_eq!(
            next.format("Y-m-d H:i:s e T").unwrap(),
            "2024-01-02 03:04:05 CST CST"
        );
        assert_eq!(
            next.start_of_day()
                .unwrap()
                .format("Y-m-d H:i:s e")
                .unwrap(),
            "2024-01-02 00:00:00 CST"
        );
        let factory = Date::get_facade_root().unwrap();
        assert_eq!(
            factory
                .parse("@-0.5", None)
                .unwrap()
                .format("Y-m-d H:i:s.uP")
                .unwrap(),
            "1969-12-31 23:59:59.500000+00:00"
        );
        assert_eq!(
            factory.raw_parse("@0", None).unwrap().format("U").unwrap(),
            "0"
        );
        let named = Date::parse(
            "2024-03-09 12:00:00",
            Some(DateValue::timezone("America/New_York").unwrap()),
        )
        .unwrap();
        let next = named
            .plus(
                openrust::support::days(1.0).unwrap(),
                openrust::support::MonthOverflow::Carry,
            )
            .unwrap();
        assert_eq!(
            next.format("Y-m-d H:i:sP e T").unwrap(),
            "2024-03-10 12:00:00-04:00 America/New_York EDT"
        );
    })
    .await;
}

#[tokio::test]
async fn explicit_native_timezone_rules_remain_the_source_for_parse_and_format() {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        let custom = DateTimezone::posix("EST5EDT,M3.2.0/2,M11.1.0/2").unwrap();
        let time = DateValue::from_utc("2024-03-09T17:00:00Z".parse().unwrap(), DateTimezone::UTC);
        Date::with_test_now(Some(time), async {
            let date = Date::parse("+1 day", Some(custom.clone())).unwrap();
            assert_eq!(
                date.format("Y-m-d H:i:sP T I").unwrap(),
                "2024-03-10 12:00:00-04:00 EDT 1"
            );
            let native = DateValue::from_utc(date.as_date_time().timestamp(), custom);
            assert_eq!(
                native.format("Y-m-d H:i:sP T I").unwrap(),
                "2024-03-10 12:00:00-04:00 EDT 1"
            );
        })
        .await
        .unwrap();
        let fixed = DateValue::from_utc(
            "2024-01-01T00:00:00Z".parse().unwrap(),
            DateTimezone::fixed(jiff::tz::Offset::from_seconds(19800).unwrap()),
        );
        assert_eq!(fixed.timezone_name(), "+05:30");
        assert_eq!(fixed.format("e T").unwrap(), "+05:30 GMT+0530");
        assert_eq!(
            Date::parse("x".repeat(20 * 1024 * 1024 + 1), None),
            Err(DateError::InputTooLarge)
        );
    })
    .await;
}

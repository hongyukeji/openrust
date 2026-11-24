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
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"America/New_York"}))].into()))
    })
    .unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn seconds_and_milliseconds_follow_all_original_php_creation_vectors_including_negative_parts()
 {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/date/timestamps-contract.json")).unwrap();
    assert_eq!(cases.len(), 120);
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        let factory = Date::get_facade_root().unwrap();
        for case in cases {
            let zone = case["timezone"]
                .as_str()
                .map(|zone| DateValue::timezone(zone).unwrap());
            let input = case["input"].clone();
            let result = match case["method"].as_str().unwrap() {
                "createFromTimestamp" => Date::create_from_timestamp(input.clone(), zone.clone()),
                "createFromTimestampUTC" => Date::create_from_timestamp_utc(input.clone()),
                "createFromTimestampMs" => {
                    Date::create_from_timestamp_ms(input.clone(), zone.clone())
                }
                "createFromTimestampMsUTC" => Date::create_from_timestamp_ms_utc(input.clone()),
                _ => unreachable!(),
            };
            let direct = match case["method"].as_str().unwrap() {
                "createFromTimestamp" => factory.create_from_timestamp(input, zone),
                "createFromTimestampUTC" => factory.create_from_timestamp_utc(input),
                "createFromTimestampMs" => factory.create_from_timestamp_ms(input, zone),
                "createFromTimestampMsUTC" => factory.create_from_timestamp_ms_utc(input),
                _ => unreachable!(),
            };
            let expected = case["result"]["date"].as_str().unwrap();
            let year = expected.split('-').next().unwrap().parse::<i32>().unwrap();
            if year > 9999 {
                assert_eq!(result, Err(DateError::OutOfRange), "{case}");
                assert_eq!(direct, Err(DateError::OutOfRange));
                continue;
            }
            let result = result.unwrap_or_else(|e| panic!("{case}: {e}"));
            assert_eq!(result.format("Y-m-d H:i:s.uP").unwrap(), expected, "{case}");
            assert_eq!(result.timezone_name(), case["result"]["zone"], "{case}");
            assert_eq!(direct.unwrap().format("Y-m-d H:i:s.uP").unwrap(), expected);
        }
    })
    .await;
}
#[tokio::test]
async fn timestamp_creation_ignores_application_timezone_until_explicitly_selected_and_rejects_invalid_shapes()
 {
    let app = app().await;
    App::set_facade_application(Some(app.app()), async {
        assert_eq!(
            Date::create_from_timestamp(0, None)
                .unwrap()
                .format("Y-m-d H:i:sP e")
                .unwrap(),
            "1970-01-01 00:00:00+00:00 +00:00"
        );
        assert_eq!(
            Date::create_from_timestamp_ms(0, Some(DateValue::timezone("Asia/Shanghai").unwrap()))
                .unwrap()
                .format("Y-m-d H:i:sP e")
                .unwrap(),
            "1970-01-01 08:00:00+08:00 Asia/Shanghai"
        );
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                Date::create_from_timestamp(value, None),
                Err(DateError::OutOfRange)
            );
            assert_eq!(
                Date::create_from_timestamp_ms(value, None),
                Err(DateError::OutOfRange)
            );
        }
        for input in [json!(null), json!(false), json!([]), json!({})] {
            assert_eq!(
                Date::create_from_timestamp(input.clone(), None),
                Err(DateError::InvalidExpression)
            );
            assert_eq!(
                Date::create_from_timestamp_ms(input, None),
                Err(DateError::InvalidExpression)
            );
        }
        assert_eq!(
            Date::create_from_timestamp(DateInput::Now, None),
            Err(DateError::InvalidExpression)
        );
        assert_eq!(
            Date::create_from_timestamp_ms("x".repeat(20 * 1024 * 1024 + 1), None),
            Err(DateError::InputTooLarge)
        );
        let custom = DateTimezone::posix("EST5EDT,M3.2.0/2,M11.1.0/2").unwrap();
        assert_eq!(
            Date::create_from_timestamp(1710072000, Some(custom))
                .unwrap()
                .format("Y-m-d H:i:sP T")
                .unwrap(),
            "2024-03-10 08:00:00-04:00 EDT"
        );
    })
    .await;
}

use openrust::{
    support::{
        Date as DateValue, DateError, DateFactory,
        facades::{App, Date, Facade},
    },
    testing::TestApp,
};
use serde_json::{Value, json};
#[tokio::test]
async fn original_time_string_constructor_keeps_modification_timezone_dst_and_error_semantics() {
    let rows: Value =
        serde_json::from_str(include_str!("fixtures/date/time-string-contract.json")).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 90);
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"Asia/Tokyo"}))].into()))
    })
    .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Date::set_test_now(Some(
            Date::parse(
                "2024-03-10 08:09:10.123456",
                Some(DateValue::timezone("America/New_York").unwrap()),
            )
            .unwrap(),
        ))
        .unwrap();
        let factory = Date::get_facade_root().unwrap();
        for row in rows.as_array().unwrap() {
            Date::use_strict_mode(row["strict"].as_bool().unwrap()).unwrap();
            let input = row["input"].as_str().unwrap();
            let zone = row["timezone"]
                .as_str()
                .map(|zone| DateValue::timezone(zone).unwrap());
            for result in [
                Date::create_from_time_string(input, zone.clone()),
                factory.create_from_time_string(input, zone),
            ] {
                if row["result"].get("error").is_some() {
                    assert_eq!(result, Err(DateError::InvalidExpression), "{row}");
                } else {
                    let date = result.unwrap();
                    assert_eq!(
                        date.format("Y-m-d H:i:s.uP").unwrap(),
                        row["result"]["date"],
                        "{row}"
                    );
                    assert_eq!(date.timezone_name(), row["result"]["zone"], "{row}");
                }
            }
        }
    })
    .await;
}
#[tokio::test]
async fn time_string_limits_and_explicit_native_timezone_are_not_swallowed_by_lenient_mode() {
    let factory = DateFactory::default();
    factory.use_strict_mode(false);
    assert_eq!(
        factory.create_from_time_string(&"a".repeat(20 * 1024 * 1024 + 1), None),
        Err(DateError::InputTooLarge)
    );
    let zone =
        openrust::support::DateTimezone::fixed(jiff::tz::Offset::from_seconds(19_800).unwrap());
    let date = factory
        .create_from_time_string("12:34 UTC", Some(zone))
        .unwrap();
    assert_eq!(date.format("H:i:sP").unwrap(), "12:34:00+05:30");
    assert!(!factory.is_strict_mode_enabled());
}

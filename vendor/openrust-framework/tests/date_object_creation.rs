use openrust::{
    support::{
        Date as DateValue, DateError, DateFactory,
        facades::{App, Date, Facade},
    },
    testing::TestApp,
};
use serde_json::{Value, json};
fn compare(result: Result<Option<DateValue>, DateError>, row: &Value) {
    let expected = &row["result"];
    if expected.get("error").is_some() {
        assert!(result.is_err(), "{row}");
    } else if expected.is_null() {
        assert!(result.unwrap().is_none(), "{row}");
    } else if expected["date"].as_str().unwrap().starts_with("10889") {
        assert_eq!(result, Err(DateError::OutOfRange));
    } else {
        let date = result.unwrap().unwrap();
        assert_eq!(
            date.format("Y-m-d H:i:s.uP").unwrap(),
            expected["date"],
            "{row}"
        );
        assert_eq!(date.timezone_name(), expected["zone"], "{row}");
    }
}
#[tokio::test]
async fn inherited_object_id_and_make_methods_match_original_and_reject_non_time_inputs() {
    let rows: Value =
        serde_json::from_str(include_str!("fixtures/date/object-creation-contract.json")).unwrap();
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"Asia/Tokyo"}))].into()))
    })
    .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        Date::set_test_now(Some(
            Date::parse(
                "2024-03-10 01:59:59.123456",
                Some(DateValue::timezone("America/New_York").unwrap()),
            )
            .unwrap(),
        ))
        .unwrap();
        let factory = Date::get_facade_root().unwrap();
        for row in rows.as_array().unwrap() {
            let (actual, direct) = match row["method"].as_str().unwrap() {
                "make" => (
                    Date::make(row["input"].clone(), None),
                    factory.make(row["input"].clone(), None),
                ),
                "id" => (
                    Date::create_from_id(row["input"].as_str().unwrap()).map(Some),
                    factory
                        .create_from_id(row["input"].as_str().unwrap())
                        .map(Some),
                ),
                method => {
                    let date = Date::parse(
                        "2024-01-02 03:04:05.123456",
                        Some(match row["timezone"].as_str().unwrap() {
                            "+05:30" => openrust::support::DateTimezone::fixed(
                                jiff::tz::Offset::from_seconds(19_800).unwrap(),
                            ),
                            zone => DateValue::timezone(zone).unwrap(),
                        }),
                    )
                    .unwrap();
                    if method == "createFromImmutable" {
                        (
                            Date::create_from_immutable(&date).map(Some),
                            factory.create_from_immutable(&date).map(Some),
                        )
                    } else {
                        (
                            Date::create_from_interface(&date).map(Some),
                            factory.create_from_interface(&date).map(Some),
                        )
                    }
                }
            };
            compare(actual, row);
            compare(direct, row);
        }
        assert_eq!(
            Date::create_from_interface("2024-01-01"),
            Err(DateError::InvalidExpression)
        );
        let original = Date::parse(
            "2024-01-02",
            Some(DateValue::timezone("America/New_York").unwrap()),
        )
        .unwrap();
        assert_eq!(
            Date::make(&original, Some(DateValue::timezone("UTC").unwrap())).unwrap(),
            Some(original.clone())
        );
        assert_eq!(
            Date::create_from_interface(original.as_date_time()).unwrap(),
            original
        );
        assert_eq!(factory.make(f64::NAN, None).unwrap(), None);
        assert_eq!(
            factory
                .make(openrust::support::DateInput::Now, None)
                .unwrap(),
            None
        );
    })
    .await;
}
#[test]
fn native_id_objects_share_string_semantics_and_do_not_read_the_application_clock() {
    let factory = DateFactory::default();
    let ulid = ulid::Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    assert_eq!(
        factory.create_from_id(ulid).unwrap(),
        factory.create_from_id(ulid.to_string()).unwrap()
    );
    let uuid = uuid::Uuid::parse_str("01890f3e-4c00-7000-8000-000000000000").unwrap();
    let borrowed = &uuid;
    assert_eq!(
        factory.create_from_id(borrowed).unwrap(),
        factory.create_from_id(uuid.to_string()).unwrap()
    );
}

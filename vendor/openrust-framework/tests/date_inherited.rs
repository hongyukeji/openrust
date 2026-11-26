use openrust::{
    support::{
        DateError, DateFactory,
        facades::{App, Date, Facade},
    },
    testing::TestApp,
};
use serde_json::{Value, json};
#[test]
fn actual_inherited_unit_queries_match_original_public_class_values_and_precision_errors() {
    let expected: Value = serde_json::from_str(include_str!(
        "fixtures/date/inherited-helpers-contract.json"
    ))
    .unwrap();
    let factory = DateFactory::default();
    assert_eq!(
        serde_json::to_value(Date::get_days().unwrap()).unwrap(),
        expected["days"]
    );
    assert_eq!(factory.get_days(), Date::get_days().unwrap());
    let mut changed = Date::get_days().unwrap();
    changed[0] = "changed".into();
    assert_eq!(Date::get_days().unwrap()[0], "Sunday");
    assert_eq!(expected["units"].as_array().unwrap().len(), 35);
    for row in expected["units"].as_array().unwrap() {
        let unit = row["unit"].as_str().unwrap();
        assert_eq!(Date::singular_unit(unit).unwrap(), row["singular"], "{row}");
        assert_eq!(factory.singular_unit(unit), row["singular"]);
        assert_eq!(Date::plural_unit(unit).unwrap(), row["plural"], "{row}");
        assert_eq!(factory.plural_unit(unit), row["plural"]);
        assert_eq!(
            Date::is_modifiable_unit(unit).unwrap(),
            row["modifiable"],
            "{row}"
        );
        assert_eq!(factory.is_modifiable_unit(unit), row["modifiable"]);
        if row["precision"].is_string() {
            assert_eq!(
                Date::get_time_format_by_precision(unit).unwrap(),
                row["precision"],
                "{row}"
            );
            assert_eq!(
                factory.get_time_format_by_precision(unit).unwrap(),
                row["precision"]
            );
        } else {
            assert_eq!(
                Date::get_time_format_by_precision(unit),
                Err(DateError::InvalidPrecision)
            );
            assert_eq!(
                factory.get_time_format_by_precision(unit),
                Err(DateError::InvalidPrecision)
            );
        }
    }
}
#[tokio::test]
async fn original_creation_probe_preserves_nullable_input_nul_errors_and_native_year_bounds() {
    let expected: Value = serde_json::from_str(include_str!(
        "fixtures/date/inherited-helpers-contract.json"
    ))
    .unwrap();
    let mut app = TestApp::create(|b| {
        b.with_config(|_, _| Ok([("app".into(), json!({"timezone":"UTC"}))].into()))
    })
    .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let factory = Date::get_facade_root().unwrap();
        for strict in [true, false] {
            Date::use_strict_mode(strict).unwrap();
            for row in expected["create"].as_array().unwrap() {
                let input = row["input"].as_str();
                let format = row["format"].as_str().unwrap();
                let result = Date::can_be_created_from_format(input, format);
                let direct = factory.can_be_created_from_format(input, format);
                if row["result"].is_object() {
                    assert_eq!(result, Err(DateError::InvalidExpression));
                    assert_eq!(direct, Err(DateError::InvalidExpression));
                } else if input == Some("1000000000000") {
                    assert!(row["result"].as_bool().unwrap());
                    assert_eq!(result, Ok(false));
                    assert_eq!(direct, Ok(false));
                } else {
                    assert_eq!(result.unwrap(), row["result"], "{row}");
                    assert_eq!(direct.unwrap(), row["result"]);
                }
            }
        }
        assert!(!Date::can_be_created_from_format(None, "Y").unwrap());
        assert!(Date::can_be_created_from_format("2024", "Y").unwrap());
    })
    .await;
}

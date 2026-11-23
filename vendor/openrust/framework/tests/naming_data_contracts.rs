use openrust::{
    database::eloquent::factories::fake,
    hashing::{HashInfo, HashOptions},
};
use serde_json::json;

#[test]
fn renamed_hash_field_keeps_the_serialized_key() {
    let info = HashInfo {
        algo: Some("fixture".into()),
        algo_name: "fixture".into(),
        options: HashOptions::new(),
    };
    assert_eq!(
        serde_json::to_value(info).unwrap(),
        json!({"algo":"fixture", "algoName":"fixture", "options":{}})
    );
}

#[test]
fn native_formatter_methods_keep_dynamic_catalog_names() {
    let faker = fake(Some("ms-MY")).unwrap();
    faker.seed(42);
    let bank = faker.local_bank().unwrap();
    faker.seed(42);
    assert_eq!(faker.format("localBank", &[]).unwrap(), bank.into());
    faker.seed(42);
    let name = faker.first_name_male_malay().unwrap();
    faker.seed(42);
    assert_eq!(
        faker.format("firstNameMaleMalay", &[]).unwrap(),
        name.into()
    );
    faker.seed(42);
    let prefix = faker.peninsular_prefix().unwrap();
    faker.seed(42);
    assert_eq!(
        faker.format("peninsularPrefix", &[]).unwrap(),
        prefix.into()
    );
    faker.seed(42);
    let processor = faker.mac_processor();
    faker.seed(42);
    assert_eq!(faker.format("macProcessor", &[]).unwrap(), processor.into());
    faker.seed(42);
    let color = faker.rgb_css_color();
    faker.seed(42);
    assert_eq!(faker.format("rgbCssColor", &[]).unwrap(), color.into());
    faker.seed(42);
    let mobile = faker.zero_one_one_prefix().unwrap();
    faker.seed(42);
    assert_eq!(
        faker.format("zeroOneOnePrefix", &[]).unwrap(),
        mobile.into()
    );
    faker.seed(42);
    let language = faker.language_code();
    faker.seed(42);
    assert_eq!(faker.format("languageCode", &[]).unwrap(), language.into());
}

#[test]
fn regional_vat_methods_stay_distinct_and_uniqueness_stays_shared() {
    let faker = fake(Some("zh-Hant")).unwrap();
    faker.seed(14);
    let numeric = faker.taiwan_vat().unwrap();
    faker.seed(14);
    assert_eq!(faker.vat(None).unwrap(), numeric.to_string());
    faker.seed(14);
    assert_eq!(faker.format("VAT", &[]).unwrap(), numeric.into());
    faker.seed(14);
    faker.unique_with_retries(true, 1).mac_processor().unwrap();
    faker.seed(14);
    assert!(faker.unique_with_retries(false, 1).mac_processor().is_err());
}

#[test]
fn zoned_period_macro_methods_preserve_bounds_and_timezone() {
    let faker = fake(Some("en-US")).unwrap();
    let max = jiff::Timestamp::now() + jiff::SignedDuration::from_hours(24);
    let zone = jiff::tz::TimeZone::get("Asia/Taipei").unwrap();
    let unique = faker.unique(true);
    for value in [
        faker.date_time_this_century_in_timezone(Some(max), Some(zone.clone())),
        faker.date_time_this_decade_in_timezone(Some(max), Some(zone.clone())),
        faker.date_time_this_year_in_timezone(Some(max), Some(zone.clone())),
        faker.date_time_this_month_in_timezone(Some(max), Some(zone.clone())),
        unique.date_time_this_century_in_timezone(Some(max), Some(zone.clone())),
        unique.date_time_this_decade_in_timezone(Some(max), Some(zone.clone())),
        unique.date_time_this_year_in_timezone(Some(max), Some(zone.clone())),
        unique.date_time_this_month_in_timezone(Some(max), Some(zone.clone())),
    ] {
        let value = value.unwrap();
        assert!(value.timestamp() <= max);
        assert_eq!(value.time_zone(), &zone);
    }
}

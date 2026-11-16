use openrust::support::{ScanError, ScanValue, Str};
use serde_json::Value;

#[test]
fn scan_matches_fixed_php_format_matrix() {
    let cases: Vec<(String, String, Value)> =
        serde_json::from_str(include_str!("fixtures/string-scan-php85.json")).unwrap();
    for (input, format, expected) in cases {
        let result = Str::of(&input).scan(&format);
        if expected.is_null() {
            assert!(result.is_err(), "{input:?}, {format:?}");
        } else {
            assert_eq!(
                result.unwrap().toArray().unwrap(),
                expected,
                "{input:?}, {format:?}"
            );
        }
    }
}

#[test]
fn scan_exposes_byte_captures_and_defers_serialization_of_nonfinite_float() {
    let fields = Str::of("甲").scan("%c%2c%n").unwrap();
    assert_eq!(fields.get(0), Some(&Some(ScanValue::Bytes(vec![0xe7]))));
    assert_eq!(
        fields.get(1),
        Some(&Some(ScanValue::Bytes(vec![0x94, 0xb2])))
    );
    assert_eq!(fields.get(2), Some(&Some(ScanValue::Integer(3))));
    let fields = Str::of("1e999").scan("%f").unwrap();
    assert_eq!(fields.get(0), Some(&Some(ScanValue::Float(f64::INFINITY))));
    assert!(fields.toJson().is_err());
    assert_eq!(Str::of("x").scan("%1$s%s"), Err(ScanError::MixedPositions));
}

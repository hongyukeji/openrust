//! 复用实际原 PHP JWT 正日期样本，发现固定六位格式化的微秒漂移。

use super::{format_php_numeric_timestamp, parse_php_numeric_timestamp};

// 词法断言来自固定 PHP 数值语法与六 ASCII 空白规则；不是新增的原生运行样本。
#[test]
fn numeric_strings_use_six_ascii_whitespace_and_existing_numeric_grammar() -> crate::Result<()> {
    for whitespace in [' ', '\t', '\n', '\r', '\u{b}', '\u{c}'] {
        let value = serde_json::json!(format!("{whitespace}+123.125{whitespace}"));
        let instant = parse_php_numeric_timestamp(&value)?;
        assert_eq!(instant.epoch_seconds(), 123);
        assert_eq!(instant.microseconds(), 125_000);
    }
    for (text, seconds, micros) in [
        (" \t\n\r\u{b}\u{c}1.25\u{c}\u{b}\r\n\t ", 1, 250_000),
        (".5", 0, 500_000),
        ("1.", 1, 0),
        ("+1.25e2", 125, 0),
        ("1E-2", 0, 10_000),
        ("-.25", 0, 250_000),
    ] {
        let instant = parse_php_numeric_timestamp(&serde_json::json!(text))?;
        assert_eq!(instant.epoch_seconds(), seconds, "{text:?}");
        assert_eq!(instant.microseconds(), micros, "{text:?}");
    }
    Ok(())
}

#[test]
fn invalid_numeric_strings_and_json_types_are_typed_errors_without_echoing_input() {
    for text in [
        "",
        " \t\n\r\u{b}\u{c}",
        "\0",
        "\u{0}1.25",
        "1.25\0",
        "\u{a0}1.25",
        "1.25\u{2003}",
        "1 2",
        "1\t2",
        "+",
        "--1",
        ".",
        "1e",
        "1e+",
        "0x10",
        "NaN",
        "INF",
        "secret-input-marker",
    ] {
        let result = parse_php_numeric_timestamp(&serde_json::json!(text));
        assert!(result.is_err(), "{text:?}");
        if let Err(error) = result {
            assert_eq!(error.code(), "input_date_format_invalid");
            assert!(!error.to_string().contains("secret-input-marker"));
            assert!(!format!("{error:?}").contains("secret-input-marker"));
        }
    }
    for value in [
        serde_json::Value::Null,
        serde_json::json!(false),
        serde_json::json!(true),
        serde_json::json!([]),
        serde_json::json!(["secret-input-marker"]),
        serde_json::json!({"secret-input-marker": 1}),
    ] {
        let result = parse_php_numeric_timestamp(&value);
        assert!(result.is_err());
        if let Err(error) = result {
            assert_eq!(error.code(), "input_date_type_invalid");
            assert!(!error.to_string().contains("secret-input-marker"));
            assert!(!format!("{error:?}").contains("secret-input-marker"));
        }
    }
}

#[test]
fn numeric_string_byte_budget_precedes_whitespace_normalization() {
    let value = serde_json::Value::String(" ".repeat(super::MAX_CALENDAR_BYTES + 1));
    let result = parse_php_numeric_timestamp(&value);
    assert!(result.is_err());
    if let Err(error) = result {
        assert_eq!(error.code(), "input_limits_exceeded");
    }
}

#[test]
fn formats_actual_jwt_positive_dates_to_six_places() -> crate::Result<()> {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("numeric_timestamp_actual.json"))
            .map_err(crate::Error::internal)?;
    let cases = fixture["cases"]
        .as_array()
        .ok_or_else(|| crate::Error::custom("fixture_invalid", "日期参考缺少样本"))?;
    assert_eq!(cases.len(), 72);
    for row in cases {
        let input = row["input"]
            .as_f64()
            .ok_or_else(|| crate::Error::custom("fixture_invalid", "日期参考不是有限数字"))?;
        let expected = row["expected"]
            .as_str()
            .ok_or_else(|| crate::Error::custom("fixture_invalid", "日期参考缺少预期"))?;
        assert_eq!(
            format_php_numeric_timestamp(input)?,
            expected,
            "{}:{}",
            row["case"],
            row["field"]
        );
    }
    Ok(())
}

#[test]
fn non_finite_values_have_explicit_range_failure() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(format_php_numeric_timestamp(value).is_err());
    }
}

#[test]
fn numeric_date_native_boundaries_keep_formatter_and_uu_stages_separate() -> crate::Result<()> {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("numeric_timestamp_boundary_actual.json"))
            .map_err(crate::Error::internal)?;
    let rows = fixture["cases"]
        .as_array()
        .ok_or_else(|| crate::Error::custom("fixture_invalid", "缺少原生日期参考"))?;
    assert_eq!(rows.len(), 36);
    let mut numeric_formats = 0;
    let mut datetime_success = 0;
    for row in rows {
        if let Some(bits) = row["float_cast"]["value"]["ieee754_binary64_be_hex"].as_str() {
            let bits = u64::from_str_radix(bits, 16).map_err(crate::Error::internal)?;
            let number = f64::from_bits(bits);
            if number.is_finite() {
                let expected = row["number_format"]["value"]
                    .as_str()
                    .ok_or_else(|| crate::Error::custom("fixture_invalid", "缺少格式化参考"))?;
                assert_eq!(
                    format_php_numeric_timestamp(number)?,
                    expected,
                    "{}",
                    row["id"]
                );
                numeric_formats += 1;
            } else {
                // 原PHP formatter返回nan/inf，U.u随后false；有限域API在此前明确拒绝。
                assert!(format_php_numeric_timestamp(number).is_err());
            }
        }
        if row["input_kind"] != "json" {
            continue;
        }
        let literal = row["input_literal"]
            .as_str()
            .ok_or_else(|| crate::Error::custom("fixture_invalid", "缺少输入参考"))?;
        let value: serde_json::Value =
            serde_json::from_str(literal).map_err(crate::Error::internal)?;
        let expected = &row["datetime"]["value"];
        if expected["result"] == "datetime" {
            let instant = parse_php_numeric_timestamp(&value)?;
            let seconds = expected["epoch_seconds"]
                .as_str()
                .ok_or_else(|| crate::Error::custom("fixture_invalid", "缺少秒参考"))?
                .parse::<i64>()
                .map_err(crate::Error::internal)?;
            let micros = expected["microseconds"]
                .as_str()
                .ok_or_else(|| crate::Error::custom("fixture_invalid", "缺少微秒参考"))?
                .parse::<u32>()
                .map_err(crate::Error::internal)?;
            assert_eq!(instant.epoch_seconds(), seconds, "{}", row["id"]);
            assert_eq!(instant.microseconds(), micros, "{}", row["id"]);
            datetime_success += 1;
        } else {
            assert!(
                parse_php_numeric_timestamp(&value).is_err(),
                "{}",
                row["id"]
            );
        }
    }
    assert_eq!(numeric_formats, 26);
    assert_eq!(datetime_success, 22);
    Ok(())
}

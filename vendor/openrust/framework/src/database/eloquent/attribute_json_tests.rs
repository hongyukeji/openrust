//! 检出局部数字词法丢失、坏Unicode被覆盖、深度边界及大文档/小栈退化。

use super::*;
use serde_json::json;

#[test]
fn php64_integer_lexemes_include_negative_zero_without_creating_unsigned_values() -> Result<()> {
    for (text, expected) in [
        ("0", 0),
        ("-0", 0),
        (" \t-0\n", 0),
        ("42", 42),
        ("9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
    ] {
        let value = decode_associative(text)?;
        assert_eq!(value.as_i64(), Some(expected));
        assert!(value.as_number().is_some_and(serde_json::Number::is_i64));
    }
    Ok(())
}

#[test]
fn floating_lexemes_keep_category_signed_zero_and_nearest_even_bits() -> Result<()> {
    for (text, bits) in [
        ("1.0", 0x3ff0_0000_0000_0000_u64),
        ("1e0", 0x3ff0_0000_0000_0000),
        ("-0.0", 0x8000_0000_0000_0000),
        ("-0e0", 0x8000_0000_0000_0000),
        ("-1e-400", 0x8000_0000_0000_0000),
        ("1e-400", 0x0000_0000_0000_0000),
        ("0e9999", 0x0000_0000_0000_0000),
        ("-0e9999", 0x8000_0000_0000_0000),
        ("9223372036854775808", 0x43e0_0000_0000_0000),
        ("-9223372036854775809", 0xc3e0_0000_0000_0000),
        ("18446744073709551615", 0x43f0_0000_0000_0000),
        ("18446744073709551616", 0x43f0_0000_0000_0000),
        ("9007199254740993.0", 0x4340_0000_0000_0000),
        ("0.1", 0x3fb9_9999_9999_999a),
        (
            "1.00000000000000011102230246251565404236316680908203125",
            0x3ff0_0000_0000_0000,
        ),
        (
            "1.00000000000000011102230246251565404236316680908203126",
            0x3ff0_0000_0000_0001,
        ),
        ("2.2250738585072014e-308", 0x0010_0000_0000_0000),
        ("5e-324", 0x0000_0000_0000_0001),
        ("2.4703282292062327e-324", 0x0000_0000_0000_0000),
        ("2.4703282292062328e-324", 0x0000_0000_0000_0001),
        ("1.7976931348623157e308", 0x7fef_ffff_ffff_ffff),
        ("1.7976931348623158e308", 0x7fef_ffff_ffff_ffff),
    ] {
        let value = decode_associative(text)?;
        assert!(value.as_number().is_some_and(serde_json::Number::is_f64));
        assert_eq!(value.as_f64().ok_or_else(cast_failed)?.to_bits(), bits);
    }
    Ok(())
}

#[test]
fn large_explicit_exponents_follow_zend_before_fractional_scale() -> Result<()> {
    let small = format!("0.{}1", "0".repeat(19_999));
    let large = format!("1{}", "0".repeat(20_000));
    // 与固定PHP8.5.12实际oracle的A/B输入一致，两条原始词法均为20008字节。
    assert_eq!(format!("{small}e20000").len(), 20_008);
    assert_eq!(format!("{large}e-20000").len(), 20_008);
    for (text, bits) in [
        (format!("{small}e19998"), 0x3f84_7ae1_47ae_147b_u64),
        (format!("{small}e19999"), 0x3fb9_9999_9999_999a),
        (format!("{small}e20000"), 0x3fb9_9999_9999_999a),
        (format!("{small}E+00000000000020000"), 0x3fb9_9999_9999_999a),
        (format!("{small}e4294967296"), 0x3fb9_9999_9999_999a),
        (format!("-{small}e20000"), 0xbfb9_9999_9999_999a),
        (format!("{large}e-19998"), 0x4059_0000_0000_0000),
        (format!("{large}e-19999"), 0x4024_0000_0000_0000),
        (format!("{large}e-20000"), 0x4024_0000_0000_0000),
        (format!("{large}e-0000000000020000"), 0x4024_0000_0000_0000),
        (format!("{large}e-4294967296"), 0x4024_0000_0000_0000),
        (format!("-{large}e-20000"), 0xc024_0000_0000_0000),
    ] {
        let value = decode_associative(&text)?;
        assert!(value.as_number().is_some_and(serde_json::Number::is_f64));
        assert_eq!(value.as_f64().ok_or_else(cast_failed)?.to_bits(), bits);
    }
    // JSON不接受后缀；Model Float只转换数值前缀，两条路径不能混用校验规则。
    assert!(decode_associative(&format!("{small}e20000suffix"))?.is_null());
    Ok(())
}

#[test]
fn complete_syntax_and_unicode_validation_precedes_any_number_conversion() -> Result<()> {
    // 对齐独立PHP8.5的php-json-edge-input/output中语法与Unicode输入；不改原fixture。
    for text in [
        "",
        r#""\uD800""#,
        r#""\uDC00""#,
        r#"{"\uD800":1}"#,
        r#"{"n":1e400,"s":"\uD800"}"#,
        r#"[1e400,"\uD800"]"#,
        r#"{"s":"\uD800","s":"ok"}"#,
        r#"{"n":1e400} garbage"#,
        r#"{"n":1e400,"n":1,}"#,
        r#"{"n":1e+}"#,
        r#""\q""#,
        "+1",
        "01",
        "Infinity",
        "-Infinity",
        "NaN",
        "[1,]",
        r#"{"a":true} false"#,
    ] {
        assert_eq!(decode_associative(text)?, Value::Null, "{text}");
    }
    assert_eq!(decode_associative(r#""\uD83D\uDE00""#)?, json!("😀"));
    assert_eq!(decode_associative(r#""Infinity""#)?, json!("Infinity"));
    assert_eq!(decode_associative("null")?, Value::Null);
    assert_eq!(decode_associative("true")?, json!(true));
    assert_eq!(decode_associative("false")?, json!(false));
    assert_eq!(decode_associative("[]")?, json!([]));
    assert_eq!(decode_associative("{}")?, json!([]));
    assert_eq!(
        decode_associative(r#"{"$serde_json::private::Number":"1e400"}"#)?,
        json!({"$serde_json::private::Number":"1e400"})
    );
    Ok(())
}

#[test]
fn duplicate_members_keep_first_position_last_value_and_validate_discarded_members() -> Result<()> {
    for (text, expected) in [
        (r#"{"n":1e400,"n":1}"#, json!({"n":1})),
        (r#"{"\u0061":1e400,"a":42}"#, json!({"a":42})),
        (r#"{"n":[1e400],"n":[1]}"#, json!({"n":[1]})),
        (r#"{"a":1,"b":2,"a":3}"#, json!({"a":3,"b":2})),
    ] {
        assert_eq!(decode_associative(text)?, expected);
    }
    let value = decode_associative(r#"{"a":1,"b":2,"\u0061":3}"#)?;
    let keys = value
        .as_object()
        .ok_or_else(cast_failed)?
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(keys, ["a", "b"]);
    let value = decode_associative(r#"{"z":1,"a":2,"\u007a":3}"#)?;
    let keys = value
        .as_object()
        .ok_or_else(cast_failed)?
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(keys, ["z", "a"]);
    // 同名早先成员不能借折叠跳过key/value/深度校验。
    assert!(decode_associative(r#"{"s":"\uD800","s":"ok"}"#)?.is_null());
    assert!(decode_associative(r#"{"\uD800":0,"a":1}"#)?.is_null());
    let too_deep = nested(512, true);
    assert!(decode_associative(&format!("{{\"a\":{too_deep},\"a\":0}}"))?.is_null());
    Ok(())
}

#[test]
fn valid_final_nonfinite_numbers_are_explicit_carrier_errors() {
    for text in [
        "1e400",
        "-1e9999",
        r#"{"n":1,"n":1e400}"#,
        r#"{"nested":[1e400]}"#,
        "1.7976931348623159e308",
    ] {
        assert!(matches!(decode_associative(text), Err(error)
            if error.code() == "model_attribute_cast_failed" && error.status().as_u16() == 500));
    }
}

fn nested(depth: usize, object: bool) -> String {
    if object {
        format!("{}0{}", "{\"x\":".repeat(depth), "}".repeat(depth))
    } else {
        format!("{}0{}", "[".repeat(depth), "]".repeat(depth))
    }
}

fn check_nested(mut value: &Value, depth: usize, object: bool) -> Result<()> {
    for _ in 0..depth {
        value = if object { value.get("x") } else { value.get(0) }.ok_or_else(cast_failed)?;
    }
    assert_eq!(value.as_i64(), Some(0));
    Ok(())
}

#[test]
fn depth_511_is_valid_and_512_is_null_for_arrays_and_objects() -> Result<()> {
    // 与独立PHP oracle的14个深度输入相同，明确越过Serde原128层限制。
    for depth in [127, 128, 129, 510, 511, 512, 513] {
        for object in [false, true] {
            let value = decode_associative(&nested(depth, object))?;
            if depth < PHP_JSON_DEPTH {
                check_nested(&value, depth, object)?;
            } else {
                assert!(value.is_null());
            }
        }
    }
    for depth in [511, 512] {
        let mut text = "0".to_owned();
        for index in (0..depth).rev() {
            text = if index % 2 == 0 {
                format!("[{text}]")
            } else {
                format!("{{\"x\":{text}}}")
            };
        }
        let value = decode_associative(&text)?;
        if depth == 511 {
            let mut current = &value;
            for index in 0..depth {
                current = if index % 2 == 0 {
                    current.get(0)
                } else {
                    current.get("x")
                }
                .ok_or_else(cast_failed)?;
            }
            assert_eq!(current.as_i64(), Some(0));
        } else {
            assert!(value.is_null());
        }
    }
    Ok(())
}

#[test]
fn wide_arrays_objects_and_long_strings_are_not_silently_size_capped() -> Result<()> {
    let numbers = std::iter::repeat_n("18446744073709551615", 20_000)
        .collect::<Vec<_>>()
        .join(",");
    let array = decode_associative(&format!("[{numbers}]"))?;
    let values = array.as_array().ok_or_else(cast_failed)?;
    assert_eq!(values.len(), 20_000);
    assert!(
        values
            .iter()
            .all(|value| value.as_f64().map(f64::to_bits) == Some(0x43f0_0000_0000_0000))
    );
    let fields = (0..8_192)
        .map(|index| format!("\"key{index}\":{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let object = decode_associative(&format!("{{{fields}}}"))?;
    let members = object.as_object().ok_or_else(cast_failed)?;
    assert_eq!(members.len(), 8_192);
    assert_eq!(members.keys().next().map(String::as_str), Some("key0"));
    assert_eq!(
        members.keys().next_back().map(String::as_str),
        Some("key8191")
    );
    assert_eq!(members.get("key8191"), Some(&json!(8_191)));
    let text = "中".repeat(120_000);
    let encoded = serde_json::to_string(&text).map_err(|_| cast_failed())?;
    assert_eq!(decode_associative(&encoded)?, Value::String(text));
    Ok(())
}

#[test]
fn bounded_deep_parse_final_value_drop_and_partial_error_cleanup_work_on_reduced_stack()
-> Result<()> {
    let worker = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| -> Result<()> {
            for object in [false, true] {
                let value = decode_associative(&nested(511, object))?;
                check_nested(&value, 511, object)?;
                // 有意执行最终公开Value自身的标准析构，不用测试helper规避该边界。
                drop(value);
                let deep = nested(510, object);
                assert!(
                    matches!(decode_associative(&format!("[{deep},1e400]")), Err(error)
                    if error.code() == "model_attribute_cast_failed")
                );
                assert!(decode_associative(&format!("[{deep},\"\\uD800\"]"))?.is_null());
            }
            Ok(())
        })
        .map_err(|_| cast_failed())?;
    worker.join().map_err(|_| cast_failed())?
}

#[test]
fn comparison_validates_duplicate_unicode_and_depth_before_using_php_arrays() -> Result<()> {
    assert!(equivalent("{\"n\":1e999,\"n\":1}", "{\"n\":1}")?);
    assert!(equivalent("{\"n\":\"\\ud800\",\"n\":1}", "null")?);
    assert!(equivalent("{\"0\":[]}", "[{}]")?);
    assert!(!equivalent("{\"00\":[]}", "[{}]")?);
    assert!(!equivalent("{\"a\":1,\"b\":2}", "{\"b\":2,\"a\":1}")?);
    let accepted = format!("{}1e999{}", "[".repeat(511), "]".repeat(511));
    let rejected = format!("[{accepted}]");
    assert!(equivalent(&accepted, &accepted)?);
    assert!(!equivalent(&accepted, "null")?);
    assert!(equivalent(&rejected, "null")?);
    Ok(())
}

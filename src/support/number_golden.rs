//! 真实 PHP 8.5 两参 clamp 的合成金标。
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "固定合成语料格式错误直接中止当前测试，生产模块不允许 panic"
)]
use super::InputNumber;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

fn number(value: &Value) -> InputNumber {
    if value["type"] == "string" {
        // PHP 弱类型边界接收数字字符串；Rust 调用方显式提供 InputNumber。
        return input(value).expect("numeric synthetic bound");
    }
    if value["type"] == "integer" {
        InputNumber::Integer(
            value["decimal"]
                .as_str()
                .expect("decimal")
                .parse()
                .expect("i64"),
        )
    } else {
        InputNumber::Float(f64::from_bits(
            u64::from_str_radix(value["bits"].as_str().expect("bits"), 16).expect("u64"),
        ))
    }
}
fn input(value: &Value) -> Option<InputNumber> {
    match value["type"].as_str().expect("type") {
        "integer" | "float" => Some(number(value)),
        "string" => InputNumber::numeric_string(
            std::str::from_utf8(
                &STANDARD
                    .decode(value["base64"].as_str().expect("base64"))
                    .expect("decode"),
            )
            .expect("utf8"),
        ),
        "boolean" | "array" | "object" | "null" => None,
        kind => panic!("unknown synthetic kind: {kind}"),
    }
}
#[test]
fn all_306_php_clamp_cases_preserve_bits_and_selected_parameter_type() {
    let mut count = 0;
    let mut rejected_by_type = 0;
    for line in include_str!("../../tests/http/fixtures/php85/clamp-golden.jsonl").lines() {
        let row: Value = serde_json::from_str(line).expect("golden");
        let args = &row["arguments"];
        if row.get("error_class").is_some() {
            // 公开 default 参数为 InputNumber；非法字符串/数组在 Rust 编译期被拒绝。
            assert_eq!(row["error_class"], "TypeError");
            assert!(matches!(
                row["id"].as_str(),
                Some("bad_default_string" | "bad_default_array")
            ));
            rejected_by_type += 1;
            continue;
        }
        let default = number(&args["default"]);
        let value = if row["present"] == false {
            default
        } else {
            input(&args["value"]).unwrap_or(default)
        };
        let result = value.clamp(number(&args["min"]), number(&args["max"]));
        let expected = number(&row["result"]);
        match (result, expected) {
            (InputNumber::Integer(actual), InputNumber::Integer(expected)) => {
                assert_eq!(actual, expected, "{}", row["id"]);
            }
            (InputNumber::Float(actual), InputNumber::Float(expected)) => {
                assert_eq!(actual.to_bits(), expected.to_bits(), "{}", row["id"]);
            }
            other => panic!("{}: wrong numeric tag {other:?}", row["id"]),
        }
        count += 1;
    }
    assert_eq!((count, rejected_by_type), (304, 2));
    eprintln!(
        "PHP85_CLAMP_GOLDEN runtime_cases={count} invalid_default_static_types={rejected_by_type} mismatches=0"
    );
}

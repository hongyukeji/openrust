//! 真实 PHP backed enum 的弱参数金标，不借用宽松 scalar cast。
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "固定合成语料格式错误直接中止当前测试，生产模块不允许 panic"
)]
use super::{EnumBacking, InputNumber};
use serde_json::{Value, json};
fn input(id: &str) -> Value {
    match id {
        "empty_array" => json!([]),
        "nonempty_array" => json!([1]),
        "object" => json!({}),
        "nonnumeric" => json!("email"),
        "numeric_string" => json!("1"),
        "zero_string" => json!("0"),
        "prefix_string" => json!("1ab"),
        "spaced_numeric" => json!(" 1 "),
        "decimal_string" => json!("1.5"),
        "exponent_string" => json!("1e0"),
        "leading_zero" => json!("01"),
        "empty_string" => json!(""),
        "int_zero" => json!(0),
        "int_one" => json!(1),
        "int_two" => json!(2),
        "float_one" => json!(1.0),
        "float_fraction" => json!(1.5),
        "negative_fraction" => json!(-0.5),
        "null" => Value::Null,
        "false" => json!(false),
        "true" => json!(true),
        _ => panic!("unknown synthetic id"),
    }
}
fn text_case(value: &str) -> Option<&'static str> {
    match value {
        "email" => Some("Email"),
        "1" => Some("One"),
        "" => Some("Empty"),
        _ => None,
    }
}
fn int_case(value: i64) -> Option<&'static str> {
    match value {
        0 => Some("Zero"),
        1 => Some("One"),
        2 => Some("Two"),
        i64::MAX => Some("Max"),
        i64::MIN => Some("Min"),
        _ => None,
    }
}
#[test]
fn php_enum_corpus_distinguishes_bags_private_nonfinite_and_php_objects() {
    let (mut bag, mut private, mut php_object) = (0, 0, 0);
    for line in include_str!("../../tests/http/fixtures/php85/enum-golden.jsonl").lines() {
        let row: Value = serde_json::from_str(line).expect("golden");
        let id = row["id"].as_str().expect("id");
        if id == "stringable" {
            php_object += 1;
            continue;
        } // PHP 对象不能成为输入 Value；Stringable 另有对象级回归。
        let text = row["enum"] == "TextChoice";
        let result = if matches!(id, "infinity" | "nan") {
            private += 1;
            let number = InputNumber::Float(if id == "nan" { f64::NAN } else { f64::INFINITY });
            if text {
                Ok(text_case(&super::weak_string_number(number)))
            } else {
                super::weak_integer(number).map(int_case)
            }
        } else {
            bag += 1;
            if text {
                String::from_input(&input(id)).map(|value| text_case(&value))
            } else {
                i64::from_input(&input(id)).map(int_case)
            }
        };
        match result {
            Ok(case) => {
                assert!(row.get("error_class").is_none(), "{id}");
                assert_eq!(case, row["case"].as_str(), "{id}");
            }
            Err(error) => {
                assert_eq!(row["error_class"], "TypeError", "{id}");
                assert_eq!(error.code(), "input_enum_type_invalid");
            }
        }
    }
    assert_eq!((bag, private, php_object), (42, 4, 2));
    eprintln!(
        "PHP85_ENUM_GOLDEN bag={bag} private_nonfinite={private} unrepresentable_php_object={php_object} mismatches=0"
    );
}

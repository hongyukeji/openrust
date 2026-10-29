//! 使用真实 PHP 8.5.0 输出校验实际 `serde_json` 输入及私有非有限转换。

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

#[test]
fn synthetic_php85_scalar_corpus_matches_real_serde_and_private_float_paths() -> crate::Result<()> {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../tests/http/fixtures/php85/scalar-cases.json"
    ))
    .map_err(crate::Error::internal)?;
    let golden = include_str!("../../tests/http/fixtures/php85/scalar-golden.jsonl")
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(crate::Error::internal)?;
    assert_eq!(cases.len(), 617);
    assert_eq!(cases.len(), golden.len());
    let mut mismatches = Vec::new();
    let mut finite = 0;
    let mut nonfinite = 0;
    let mut json_decode_difference = 0;
    let mut json_uint_representation = 0;
    for (case, expected) in cases.iter().zip(&golden) {
        assert_eq!(case["id"], expected["id"]);
        let id = case["id"].as_str().unwrap_or("unlabeled");
        let value = if case["kind"] == "json" {
            match serde_json::from_str(case["raw"].as_str().unwrap_or("null")) {
                Ok(value) => value,
                Err(error) => {
                    // PHP json_decode 接受指数溢出为 INF；serde_json::Value 明确拒绝。
                    // 这不是公开 bag 的转换通过项，单独证明解码边界和私有转换。
                    assert_eq!(id, "json_overflow");
                    assert!(error.is_syntax());
                    assert!(error.to_string().starts_with("number out of range"));
                    assert_eq!(expected["input"]["bits"], "7ff0000000000000");
                    check_private_float(id, f64::INFINITY, expected, &mut mismatches);
                    json_decode_difference += 1;
                    continue;
                }
            }
        } else {
            let bits = u64::from_str_radix(case["bits"].as_str().unwrap_or("0"), 16)
                .map_err(crate::Error::internal)?;
            let number = f64::from_bits(bits);
            if !number.is_finite() {
                nonfinite += 1;
                check_private_float(id, number, expected, &mut mismatches);
                continue;
            }
            Value::from(number)
        };
        if matches!(id, "uint_above_i64" | "uint_max") {
            assert!(value.is_u64());
            assert_eq!(expected["input"]["type"], "float");
            json_uint_representation += 1;
        }
        finite += 1;
        if expected["string"]["base64"] != STANDARD.encode(super::string(&value)) {
            mismatches.push(format!("{id}:string"));
        }
        if expected["integer"]["decimal"]
            .as_str()
            .and_then(|decimal| decimal.parse::<i64>().ok())
            != Some(super::integer(&value))
        {
            mismatches.push(format!("{id}:integer"));
        }
        if expected["float"]["bits"] != format!("{:016x}", super::float(&value).to_bits()) {
            mismatches.push(format!("{id}:float"));
        }
        if expected["boolean"]["value"] != super::boolean(&value) {
            mismatches.push(format!("{id}:boolean"));
        }
    }
    assert_eq!(json_uint_representation, 2);
    eprintln!(
        "PHP85_SCALAR_GOLDEN cases={} serde_finite={} private_nonfinite={} json_decode_difference={} uint_representation_difference={} mismatches={}",
        cases.len(),
        finite,
        nonfinite,
        json_decode_difference,
        json_uint_representation,
        mismatches.len()
    );
    assert!(mismatches.is_empty(), "{mismatches:?}");
    Ok(())
}

fn check_private_float(id: &str, number: f64, expected: &Value, mismatches: &mut Vec<String>) {
    if expected["integer"]["decimal"]
        .as_str()
        .and_then(|decimal| decimal.parse::<i64>().ok())
        != Some(super::double_integer(number))
    {
        mismatches.push(format!("{id}:private_integer"));
    }
    if expected["string"]["base64"] != STANDARD.encode(super::double_string(number)) {
        mismatches.push(format!("{id}:private_string"));
    }
    assert_eq!(
        expected["float"]["bits"],
        format!("{:016x}", number.to_bits())
    );
    assert_eq!(expected["boolean"]["value"], false);
}

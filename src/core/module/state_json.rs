//! 文件激活器的顶层 PHP 关联数组形状与脱敏 JSON 编解码。
//!
//! 只在状态表顶层对应 PHP array/list；未知值保留原 JSON 结构。
//! 输出采用四空格缩进，不模拟 PHP 的 Unicode/字节转义或任意 `serialize_precision`。
//! `serde_json` 无法表示的非有限数字仍在解码时明确失败。

use serde::Serialize;
use serde::ser::{SerializeSeq, Serializer};
use serde_json::{Map, Value};

use crate::core::{Error, Result};

/// 读取普通状态文件；非空 PHP list 不是模块名称表，不能自动恢复为空表。
pub(super) fn decode(bytes: &[u8]) -> Result<Map<String, Value>> {
    decode_inner(bytes, false)
}

/// 恢复本实例最后成功写入的列表；调用者必须先证明输入字节与该次写入完全相同。
///
/// 此函数本身不验证文件归属或提交记录，不能用于普通外部文件的宽松读取。
/// 只有非空 JSON array 形状获得此例外；JSON object 仍遵守普通状态表检查。
pub(super) fn decode_owned_list(bytes: &[u8]) -> Result<Map<String, Value>> {
    decode_inner(bytes, true)
}

fn decode_inner(bytes: &[u8], owned_list: bool) -> Result<Map<String, Value>> {
    // 固定 FileActivator 在 json_decode 前仅判断 PHP trim 后是否为空；
    // 默认 trim 包含 NUL/VT，却不包含 FF，不能用 ASCII whitespace 集合替代。
    if bytes
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'\x0b' | b'\0'))
    {
        return Ok(Map::new());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| invalid_state())?;
    match value {
        Value::Object(entries) if entries.is_empty() || !is_php_list(&entries) => Ok(entries),
        Value::Array(values) if values.is_empty() || owned_list => Ok(values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect()),
        _ => Err(invalid_state()),
    }
}

/// 按顶层 PHP `JSON_PRETTY_PRINT` 形状写出；空表及连续数字键写为 array。
///
/// 未知值保持原 Value，不递归把其中的 JSON object 改成 array。
pub(super) fn encode(entries: &Map<String, Value>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    if is_php_list(entries) {
        let mut sequence = serializer
            .serialize_seq(Some(entries.len()))
            .map_err(|_| invalid_state())?;
        for value in entries.values() {
            sequence
                .serialize_element(value)
                .map_err(|_| invalid_state())?;
        }
        sequence.end().map_err(|_| invalid_state())?;
    } else {
        entries
            .serialize(&mut serializer)
            .map_err(|_| invalid_state())?;
    }
    Ok(bytes)
}

fn is_php_list(entries: &Map<String, Value>) -> bool {
    // PHP array_is_list 只接受按序的整数键0..n-1。因此精确匹配规范非负
    // 十进制已足够；前导零、正号、负零、负键和整数溢出键均不能命中。
    entries
        .keys()
        .enumerate()
        .all(|(index, key)| i64::try_from(index).is_ok_and(|index| *key == index.to_string()))
}

fn invalid_state() -> Error {
    Error::custom(
        "module_state_invalid",
        "模块状态文件不是有效的模块名称 JSON 对象",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entries(values: impl IntoIterator<Item = (&'static str, Value)>) -> Map<String, Value> {
        values
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect()
    }

    #[test]
    fn numeric_objects_are_lists_only_when_keys_have_exact_order_and_spelling() -> Result<()> {
        for input in [br#"{"0":true}"#.as_slice(), br#"{"0":true,"1":false}"#] {
            assert!(decode(input).is_err());
            assert!(decode_owned_list(input).is_err());
        }
        let reversed = decode(br#"{"1":false,"0":true}"#)?;
        assert_eq!(
            reversed.keys().map(String::as_str).collect::<Vec<_>>(),
            ["1", "0"]
        );
        assert_eq!(reversed.get("0"), Some(&Value::Bool(true)));
        assert_eq!(decode_owned_list(br#"{"1":false,"0":true}"#)?, reversed);
        assert!(decode(br#"{"0":true,"2":false}"#).is_ok());
        assert!(decode(br#"{"1":true}"#).is_ok());
        Ok(())
    }

    #[test]
    fn non_list_numeric_looking_keys_stay_literal_and_keep_insertion_order() -> Result<()> {
        let keys = [
            "-1",
            "00",
            "-0",
            "+0",
            " 0",
            "9223372036854775807",
            "9223372036854775808",
            "-9223372036854775809",
        ];
        for key in keys {
            let original = entries([(key, json!({"unknown": [null, 7]}))]);
            let encoded = encode(&original)?;
            assert_eq!(decode(&encoded)?, original);
            assert_eq!(decode_owned_list(&encoded)?, original);
            assert_eq!(encoded.first(), Some(&b'{'));
        }
        let original = entries([("-1", json!(true)), ("0", json!(false))]);
        assert_eq!(decode(&encode(&original)?)?, original);
        Ok(())
    }

    #[test]
    fn empty_inputs_accept_php_trim_blanks_and_empty_container_shapes() -> Result<()> {
        for input in [b"".as_slice(), b" \r\n\t\x0b\0", b"{}", b"[]", b" \n[]\t"] {
            assert!(decode(input)?.is_empty());
            assert!(decode_owned_list(input)?.is_empty());
        }
        for input in [
            b"\x0c".as_slice(),
            b"null",
            b"true",
            b"false",
            b"0",
            b"\"\"",
            b"[true]",
            b"[null, false]",
            b"{}\0",
        ] {
            assert!(decode(input).is_err());
        }
        for input in [b"null".as_slice(), b"true", b"0", b"\"\"", b"\x0c"] {
            assert!(decode_owned_list(input).is_err());
        }
        Ok(())
    }

    #[test]
    fn owned_list_recovery_is_separate_and_keeps_unknown_nested_shapes() -> Result<()> {
        let input = br#"[true,{"0":"nested-object"},[false,null]]"#;
        assert!(decode(input).is_err());
        let restored = decode_owned_list(input)?;
        assert_eq!(
            restored.keys().map(String::as_str).collect::<Vec<_>>(),
            ["0", "1", "2"]
        );
        assert_eq!(restored.get("1"), Some(&json!({"0": "nested-object"})));
        assert_eq!(restored.get("2"), Some(&json!([false, null])));
        let output = encode(&restored)?;
        assert!(decode(&output).is_err());
        assert_eq!(decode_owned_list(&output)?, restored);
        Ok(())
    }

    #[test]
    fn encoding_uses_php_top_level_shape_and_four_space_pretty_output() -> Result<()> {
        assert_eq!(encode(&Map::new())?, b"[]");
        let single = entries([("0", json!(true))]);
        assert_eq!(encode(&single)?, b"[\n    true\n]");
        let list = entries([("0", json!(true)), ("1", json!(false))]);
        assert_eq!(encode(&list)?, b"[\n    true,\n    false\n]");
        let object = entries([("1", json!(false)), ("0", json!(true))]);
        assert_eq!(
            encode(&object)?,
            b"{\n    \"1\": false,\n    \"0\": true\n}"
        );
        assert_eq!(decode(&encode(&object)?)?, object);
        Ok(())
    }

    #[test]
    fn ordinary_objects_keep_unknown_values_without_recursive_php_shape_conversion() -> Result<()> {
        let original = entries([
            ("Blog", json!(true)),
            ("blog", json!(false)),
            ("Unowned", json!({"0": "值/🌋", "1": [null, false, 9.5]})),
            ("Nullable", Value::Null),
            ("RawArray", json!(["0", {"0": true}])),
        ]);
        let decoded = decode(&encode(&original)?)?;
        assert_eq!(decoded, original);
        assert_eq!(
            decoded.keys().map(String::as_str).collect::<Vec<_>>(),
            ["Blog", "blog", "Unowned", "Nullable", "RawArray"]
        );
        assert!(decoded["Unowned"].is_object());
        assert!(decoded["RawArray"].is_array());
        Ok(())
    }

    #[test]
    fn invalid_json_and_unrepresentable_values_share_a_redacted_error() {
        for input in [
            br#"{"private-secret-key":"private-secret-value",}"#.as_slice(),
            br#"{"private-secret-key":1e999}"#,
            br#"{"private-secret-key":NaN}"#,
            b"\xffprivate-secret-value",
            b"[true]",
            b"null",
        ] {
            let result = decode(input);
            assert!(result.is_err(), "非法状态必须失败");
            if let Err(error) = result {
                assert_eq!(error.code(), "module_state_invalid");
                assert!(!error.to_string().contains("private-secret"));
                assert!(!format!("{error:?}").contains("private-secret"));
            }
        }
    }
}

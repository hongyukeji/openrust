//! 请求 URL 的查询串规则；不修改受管理请求的输入 bag。
//!
//! canonical 对照 Symfony `HttpFoundation` 7.4.0/8.0.0 的 `HeaderUtils::parseQuery`
//! 与 `Request::normalizeQueryString`；增删参数使用 `Arr::query/forget` 的对应语义。

use std::cmp::Ordering;

use serde_json::{Map, Value};

use crate::support::{
    InputNumber, data,
    php_variables::{Registration, VariableBag},
    query_encoding::{MAX_OUTPUT_BYTES, entries, invalid, merge_arrays, validate},
};
use crate::{Error, Result};

const MAX_RAW_BYTES: usize = 16 * 1024;
const MAX_FIELDS: usize = 1024;

/// 从未经过 `TrimStrings` 的原始查询串生成规范形式。
pub(super) fn canonical(raw: Option<&str>) -> Result<String> {
    let raw = raw.unwrap_or_default();
    if raw.len() > MAX_RAW_BYTES {
        return Err(data::limit_error());
    }
    let mut bag = VariableBag::default();
    for (index, pair) in raw.split('&').filter(|pair| !pair.is_empty()).enumerate() {
        if index >= MAX_FIELDS {
            return Err(data::limit_error());
        }
        // HeaderUtils 在每个字段内截断原始 NUL；后面的 & 字段仍须处理。
        let pair = pair.split('\0').next().unwrap_or_default();
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let mut decoded = decode_bytes(key);
        if let Some(end) = decoded.iter().position(|byte| *byte == 0) {
            decoded.truncate(end);
        }
        let decoded = String::from_utf8(decoded).map_err(|_| encoding_error())?;
        let key = decoded.trim_start_matches(' ');
        let first = key.find('[').unwrap_or(key.len());
        let mut shielded = String::with_capacity(key.len().saturating_mul(2));
        for byte in key[..first].bytes() {
            shielded.push(char::from(HEX_LOWER[usize::from(byte >> 4)]));
            shielded.push(char::from(HEX_LOWER[usize::from(byte & 15)]));
        }
        shielded.push_str(&key[first..]);
        // 根键已保护；仍使用 PHP parse_str 的原有嵌套前缀过滤规则。
        bag.insert(&shielded, decode(value)?, Registration::Input)?;
    }
    let mut restored = Map::new();
    for (key, value) in entries(&bag.into_value())? {
        restored.insert(unshield(&key)?, value.clone());
    }
    let mut sorted = restored
        .into_iter()
        .enumerate()
        .map(|(index, (key, value))| (index, key, value))
        .collect::<Vec<_>>();
    sort_keys(&mut sorted);
    encode(&Value::Object(
        sorted
            .into_iter()
            .map(|(_, key, value)| (key, value))
            .collect(),
    ))
}

pub(super) use crate::support::query_encoding::encode;

/// `array_merge` 仅合并顶层：字符串键后写覆盖，整数键顺序重新编号。
pub(super) fn with_query(current: &Value, additional: &Value) -> Result<String> {
    encode(&merge_arrays(current, additional)?)
}

/// `Arr::except` 复用唯一输入树的精确键优先与非重编号删除规则。
pub(super) fn without_query(current: &Value, keys: &[&str]) -> Result<(String, bool)> {
    validate(current)?;
    if keys.len() > MAX_FIELDS {
        return Err(data::limit_error());
    }
    if keys
        .iter()
        .try_fold(0_usize, |total, key| total.checked_add(key.len()))
        .is_none_or(|total| total > MAX_OUTPUT_BYTES)
    {
        return Err(data::limit_error());
    }
    let mut remaining = current.clone();
    for key in keys {
        data::forget(&mut remaining, key)?;
    }
    let nonempty = match &remaining {
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        _ => false,
    };
    Ok((encode(&remaining)?, nonempty))
}

const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode(text: &str) -> Result<String> {
    String::from_utf8(decode_bytes(text)).map_err(|_| encoding_error())
}

fn decode_bytes(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(cursor + 1).and_then(|v| hex(*v)),
                bytes.get(cursor + 2).and_then(|v| hex(*v)),
            )
        {
            decoded.push(high * 16 + low);
            cursor += 3;
        } else {
            decoded.push(if bytes[cursor] == b'+' {
                b' '
            } else {
                bytes[cursor]
            });
            cursor += 1;
        }
    }
    decoded
}

fn unshield(key: &str) -> Result<String> {
    let (root, suffix) = key
        .split_once('_')
        .map_or((key, None), |(root, suffix)| (root, Some(suffix)));
    let mut decoded = Vec::with_capacity(root.len() / 2);
    for pair in root.as_bytes().as_chunks::<2>().0 {
        let high = hex(pair[0]).ok_or_else(invalid)?;
        let low = hex(pair[1]).ok_or_else(invalid)?;
        decoded.push(high * 16 + low);
    }
    let mut root = String::from_utf8(decoded).map_err(|_| encoding_error())?;
    if let Some(suffix) = suffix {
        root.push('[');
        root.push_str(suffix);
    }
    Ok(root)
}

fn integer_key(key: &str) -> Option<i64> {
    key.parse::<i64>()
        .ok()
        .filter(|integer| integer.to_string() == key)
}

type SortEntry = (usize, String, Value);

fn greater(values: &[SortEntry], left: usize, right: usize) -> bool {
    compare_keys(&values[left].1, &values[right].1)
        .then_with(|| values[left].0.cmp(&values[right].0))
        .is_gt()
}

// PHP 8.5 zend_sort 的比较顺序也是混合非全序键的可观察行为，不能交给要求
// 全序的 Rust sort。这里只复现有界索引上的比较/分区规则，不复制其指针实现。
fn sort_keys(values: &mut [SortEntry]) {
    let mut pending = vec![(0, values.len())];
    while let Some((start, end)) = pending.pop() {
        let count = end - start;
        if count <= 16 {
            sort_small(&mut values[start..end]);
            continue;
        }
        let middle = start + count / 2;
        if count >= 1024 {
            let delta = (count / 2) / 2;
            sort_sample(
                values,
                &[start, start + delta, middle, middle + delta, end - 1],
            );
        } else {
            sort_sample(values, &[start, middle, end - 1]);
        }
        let pivot = start + 1;
        values.swap(pivot, middle);
        let mut left = pivot + 1;
        let mut right = end - 1;
        'partition: loop {
            while greater(values, pivot, left) {
                left += 1;
                if left == right {
                    break 'partition;
                }
            }
            right -= 1;
            if right == left {
                break;
            }
            while greater(values, right, pivot) {
                right -= 1;
                if right == left {
                    break 'partition;
                }
            }
            values.swap(left, right);
            left += 1;
            if left == right {
                break;
            }
        }
        values.swap(pivot, left - 1);
        pending.push((start, left - 1));
        pending.push((left, end));
    }
}

fn sort_sample(values: &mut [SortEntry], sample: &[usize]) {
    let [a, b, c, rest @ ..] = sample else {
        return;
    };
    if greater(values, *a, *b) {
        if greater(values, *c, *b) {
            values.swap(*a, *b);
            if greater(values, *b, *c) {
                values.swap(*b, *c);
            }
        } else {
            values.swap(*a, *c);
        }
    } else if greater(values, *b, *c) {
        values.swap(*b, *c);
        if greater(values, *a, *b) {
            values.swap(*a, *b);
        }
    }
    for offset in 0..rest.len() {
        let mut position = offset + 3;
        while position > 0 && greater(values, sample[position - 1], sample[position]) {
            values.swap(sample[position - 1], sample[position]);
            position -= 1;
        }
    }
}

fn sort_small(values: &mut [SortEntry]) {
    if values.len() <= 1 {
        return;
    }
    if values.len() == 2 {
        if greater(values, 0, 1) {
            values.swap(0, 1);
        }
        return;
    }
    if values.len() <= 5 {
        sort_sample(values, &(0..values.len()).collect::<Vec<_>>());
        return;
    }
    for index in 1..values.len() {
        let mut destination = index - 1;
        if !greater(values, destination, index) {
            continue;
        }
        if index < 6 {
            while destination > 0 {
                destination -= 1;
                if !greater(values, destination, index) {
                    destination += 1;
                    break;
                }
            }
        } else {
            loop {
                destination -= 2;
                if !greater(values, destination, index) {
                    destination += 1;
                    if !greater(values, destination, index) {
                        destination += 1;
                    }
                    break;
                }
                if destination == 0 {
                    break;
                }
                if destination == 1 {
                    destination = usize::from(greater(values, index, 0));
                    break;
                }
            }
        }
        for position in (destination + 1..=index).rev() {
            values.swap(position - 1, position);
        }
    }
}

#[allow(
    clippy::float_cmp,
    reason = "固定 PHP smart_strcmp 对相等 double 与溢出作精确分支"
)]
fn compare_keys(left: &str, right: &str) -> Ordering {
    let a = InputNumber::numeric_string(left);
    let b = InputNumber::numeric_string(right);
    match (a, b) {
        (Some(InputNumber::Integer(a)), Some(InputNumber::Integer(b))) => a.cmp(&b),
        (Some(a), Some(b)) => {
            let a_float = a.as_float();
            let b_float = b.as_float();
            if integer_key(left).is_none() && integer_key(right).is_none() {
                // zend_smart_strcmp 对两个同向溢出整数或无穷值回退字节比较；
                // 真正的整数键与字符串比较则直接采用 double 规则。
                let overflow = |text: &str| {
                    let text = text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
                    !text.contains(['.', 'e', 'E']) && text.parse::<i64>().is_err()
                };
                if a_float == b_float
                    && ((!a_float.is_finite()) || (overflow(left) && overflow(right)))
                {
                    return left.cmp(right);
                }
                if matches!(a, InputNumber::Integer(_)) && overflow(right) {
                    return if b_float.is_sign_negative() {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    };
                }
                if matches!(b, InputNumber::Integer(_)) && overflow(left) {
                    return if a_float.is_sign_negative() {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }
            }
            a_float.partial_cmp(&b_float).unwrap_or(Ordering::Equal)
        }
        _ => left.cmp(right),
    }
}

fn encoding_error() -> Error {
    Error::custom_with_status(
        http::StatusCode::BAD_REQUEST,
        "input_encoding_invalid",
        "URL查询参数必须使用有效 UTF-8",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize, PartialEq)]
    struct CanonicalReference {
        id: String,
        raw: String,
        expected: String,
        #[serde(default)]
        rust_adaptation: Option<String>,
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct OperationReference {
        id: String,
        operation: String,
        current: Value,
        #[serde(default)]
        additional: Value,
        #[serde(default)]
        keys: Vec<String>,
        expected: String,
        has_remaining: bool,
    }

    #[derive(Deserialize)]
    struct Reference {
        metadata: Value,
        canonical: Vec<CanonicalReference>,
        operations: Vec<OperationReference>,
        warnings: Vec<Value>,
    }

    /// 直接读取真实 PHP 输出逐 ID 比较；原始 NUL 只验证私有 helper，
    /// 不表示 HTTP URI 接受 NUL。非法 UTF-8 单独断言 Rust 的已公开适配。
    #[test]
    fn canonical_and_array_operations_match_real_php85_symfony_both_lines() -> Result<()> {
        let first: Reference = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/laravel-request-url/golden-v7.4.0.json"
        ))
        .map_err(Error::internal)?;
        let second: Reference = serde_json::from_str(include_str!(
            "../../../tests/http/fixtures/laravel-request-url/golden-v8.0.0.json"
        ))
        .map_err(Error::internal)?;
        assert_eq!(first.canonical, second.canonical);
        assert_eq!(first.operations, second.operations);
        let mut mismatch_count = 0_usize;
        let mut examples = Vec::new();
        for reference in [&first, &second] {
            assert_eq!(reference.metadata["php_version"], "8.5.0");
            assert_eq!(reference.metadata["max_input_vars"], "4096");
            assert_eq!(reference.warnings, Vec::<Value>::new());
            assert_eq!(reference.canonical.len(), 198);
            assert_eq!(reference.operations.len(), 7);
            let mut adaptations = 0;
            let mut identifiers = std::collections::BTreeSet::new();
            for case in &reference.canonical {
                assert!(identifiers.insert(&case.id), "重复ID {}", case.id);
                adaptations += usize::from(case.rust_adaptation.is_some());
                collect_mismatch(canonical_mismatch(case), &mut mismatch_count, &mut examples);
            }
            assert_eq!(adaptations, 2);
            for case in &reference.operations {
                assert!(identifiers.insert(&case.id), "重复ID {}", case.id);
                collect_mismatch(operation_mismatch(case), &mut mismatch_count, &mut examples);
            }
        }
        assert_eq!(
            mismatch_count,
            0,
            "两条固定线共{mismatch_count}项差异，仅列前10项：\n{}",
            examples.join("\n")
        );
        Ok(())
    }

    fn collect_mismatch(mismatch: Option<String>, count: &mut usize, examples: &mut Vec<String>) {
        if let Some(detail) = mismatch {
            *count += 1;
            if examples.len() < 10 {
                examples.push(detail);
            }
        }
    }

    fn preview(value: &str) -> String {
        let prefix = value.chars().take(160).collect::<String>();
        format!("{prefix:?} ({} bytes)", value.len())
    }

    fn canonical_mismatch(case: &CanonicalReference) -> Option<String> {
        let actual = canonical(Some(&case.raw));
        match (&case.rust_adaptation, &actual) {
            (Some(category), Err(error))
                if category == "input_encoding_invalid"
                    && error.code() == category
                    && error.status() == http::StatusCode::BAD_REQUEST =>
            {
                None
            }
            (None, Ok(text)) if text == &case.expected => None,
            _ => {
                let observed = actual.as_ref().map_or_else(
                    |error| format!("error {}/{}", error.code(), error.status()),
                    |text| preview(text),
                );
                let expected = case.rust_adaptation.as_ref().map_or_else(
                    || preview(&case.expected),
                    |category| format!("error {category}/400"),
                );
                Some(format!("{} actual={observed} expected={expected}", case.id))
            }
        }
    }

    fn operation_mismatch(case: &OperationReference) -> Option<String> {
        let actual = match case.operation.as_str() {
            "encode" => encode(&case.current).map(|text| (text, None)),
            "merge" => with_query(&case.current, &case.additional).map(|text| (text, None)),
            "without" => {
                let keys = case.keys.iter().map(String::as_str).collect::<Vec<_>>();
                without_query(&case.current, &keys).map(|(text, remaining)| (text, Some(remaining)))
            }
            _ => return Some(format!("{} unknown operation", case.id)),
        };
        match actual {
            Ok((text, remaining))
                if text == case.expected
                    && remaining.is_none_or(|value| value == case.has_remaining) =>
            {
                None
            }
            Ok((text, remaining)) => Some(format!(
                "{} actual={} remaining={remaining:?} expected={} remaining={}",
                case.id,
                preview(&text),
                preview(&case.expected),
                case.has_remaining
            )),
            Err(error) => Some(format!(
                "{} error {}/{}",
                case.id,
                error.code(),
                error.status()
            )),
        }
    }

    // 固定 Symfony RequestTest::getQueryStringNormalizationData 的行为候选；
    // 这是 Rust 回归测试，不冒充已执行官方 PHP 测试的差分金标。
    #[test]
    fn canonical_matches_official_normalization_cases() -> Result<()> {
        for (raw, expected) in [
            ("foo", "foo="),
            ("foo=bar&bar=", "bar=&foo=bar"),
            ("baz=Foo%20Baz&bar=Foo+Bar", "bar=Foo%20Bar&baz=Foo%20Baz"),
            ("foo[]=1&foo[]=2", "foo%5B0%5D=1&foo%5B1%5D=2"),
            ("foo=1&foo=2", "foo=2"),
            ("Foo Bar&Foo%20Baz", "Foo%20Bar=&Foo%20Baz="),
            ("foo=bar&&&test&&", "foo=bar&test="),
            ("foo=bar&=a=b&=x=y", "foo=bar"),
            ("foo[Z]=B&foo[A]=B", "foo%5BZ%5D=B&foo%5BA%5D=B"),
            ("utf8=✓", "utf8=%E2%9C%93"),
        ] {
            assert_eq!(canonical(Some(raw))?, expected, "{raw}");
        }
        assert_eq!(canonical(None)?, "");
        Ok(())
    }

    #[test]
    fn shielding_preserves_names_malformed_brackets_and_per_field_nul() -> Result<()> {
        for (raw, expected) in [
            ("a.b=c&a+b=d", "a%20b=d&a.b=c"),
            ("a[b=c", "a%5Bb=c"),
            ("a]b=c", "a%5Db=c"),
            ("a[b][c.d]=c", "a%5Bb%5D%5Bc.d%5D=c"),
            ("a\0b=c&z=ok", "a=&z=ok"),
            ("a%00b=c&z=ok", "a=c&z=ok"),
            ("a%00%FF=c&z=ok", "a=c&z=ok"),
            ("a=x%00y&z=ok", "a=x%00y&z=ok"),
            ("a[__Host-x]=1&a[__Secure-x]=2", ""),
        ] {
            assert_eq!(canonical(Some(raw))?, expected, "{raw:?}");
        }
        Ok(())
    }

    #[test]
    fn canonical_sorts_only_top_level_with_php_numeric_keys() -> Result<()> {
        assert_eq!(
            canonical(Some("10=a&2=b&02=c&-1=d&x[Z]=1&x[A]=2"))?,
            "-1=d&2=b&02=c&10=a&x%5BZ%5D=1&x%5BA%5D=2"
        );
        // PHP SORT_REGULAR 非全序的三键按 zend_sort_3 的真实比较次序排列。
        assert_eq!(canonical(Some("10=a&2=b&1a=c"))?, "1a=c&2=b&10=a");
        Ok(())
    }

    #[test]
    fn sorting_exercises_small_partition_and_maximum_field_sample() -> Result<()> {
        for count in [0_usize, 1, 2, 3, 4, 5, 6, 16, 17, 32, 1024] {
            let fields = (0..count)
                .map(|index| format!("k{index:04}={index}"))
                .collect::<Vec<_>>();
            let raw = fields.iter().rev().cloned().collect::<Vec<_>>().join("&");
            assert_eq!(canonical(Some(&raw))?, fields.join("&"), "count={count}");
        }
        Ok(())
    }

    #[test]
    fn merge_overwrites_whole_child_and_reindexes_only_integer_keys() -> Result<()> {
        let current =
            json!({"9": "old", "nested": {"old": 1}, "01": "literal", "-0": "negative-zero"});
        let additional = json!({"-2": "new", "nested": {"new": 2}, "+1": "plus"});
        assert_eq!(
            with_query(&current, &additional)?,
            "0=old&nested%5Bnew%5D=2&01=literal&-0=negative-zero&1=new&%2B1=plus"
        );
        assert_eq!(with_query(&json!(["x"]), &json!(["y"]))?, "0=x&1=y");
        assert!(with_query(&json!({}), &json!("bad")).is_err());
        Ok(())
    }

    #[test]
    fn deletion_prefers_exact_keys_and_keeps_sparse_array_indices() -> Result<()> {
        let current = json!({"a.b": "literal", "a": {"b": "nested"}, "items": ["x", "y", "z"]});
        assert_eq!(
            without_query(&current, &["a.b", "items.1"])?.0,
            "a%5Bb%5D=nested&items%5B0%5D=x&items%5B2%5D=z"
        );
        assert_eq!(
            without_query(&current, &["a.b", "a.b"])?.0,
            "items%5B0%5D=x&items%5B1%5D=y&items%5B2%5D=z"
        );
        assert_eq!(
            without_query(&json!({"a": null}), &[])?,
            (String::new(), true)
        );
        assert_eq!(
            without_query(&json!({"a": null}), &["a"])?,
            (String::new(), false)
        );
        Ok(())
    }

    #[test]
    fn encoding_omits_null_and_empty_arrays_and_uses_php_bool_float() -> Result<()> {
        assert_eq!(
            encode(
                &json!({"null": null, "empty": [], "false": false, "true": true, "float": 1.0e20, "space": " +~"})
            )?,
            "false=0&true=1&float=1.0E%2B20&space=%20%2B~"
        );
        Ok(())
    }

    #[test]
    fn invalid_utf8_and_budgets_fail_without_lossy_conversion() {
        assert!(canonical(Some("bad=%FF")).is_err());
        assert!(canonical(Some("%FF=x")).is_err());
        assert!(canonical(Some(&"x".repeat(MAX_RAW_BYTES + 1))).is_err());
        let fields = std::iter::repeat_n("a=x", MAX_FIELDS + 1)
            .collect::<Vec<_>>()
            .join("&");
        assert!(canonical(Some(&fields)).is_err());
        let nested = format!("a{}=x", "[b]".repeat(65));
        assert!(canonical(Some(&nested)).is_err());
        let expanded = json!({"a": " ".repeat(MAX_OUTPUT_BYTES / 3 + 1)});
        assert!(encode(&expanded).is_err());
    }
}

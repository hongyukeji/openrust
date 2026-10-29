//! 请求输入 bag 专用的 PHP 数组与 Laravel 路径语义。

use serde_json::{Map, Value};

const MAX_DEPTH: usize = 64;
const MAX_FIELDS: usize = 1024;
const MAX_EXPANDED_KEYS: usize = 20 * 1024 * 1024;

/// 仅供输入 bag：按 `data_get` 读取，通配缺失项保留 null。
/// 私有保护把超限路径视为缺失；公开 Request 入口须先校验并返回 413。
pub(crate) fn get(value: &Value, key: &str) -> Option<Value> {
    if key.len() > MAX_EXPANDED_KEYS {
        return None;
    }
    // 即使内部调用漏了公开入口检查，也只收集至多 65 个片段。
    let parts = key.split('.').take(MAX_DEPTH + 1).collect::<Vec<_>>();
    if parts.len() > MAX_DEPTH {
        return None;
    }
    read(value, &parts)
}

fn read(value: &Value, parts: &[&str]) -> Option<Value> {
    let Some((&part, rest)) = parts.split_first() else {
        return Some(value.clone());
    };
    if part == "*" {
        let items: Vec<_> = match value {
            Value::Array(values) => values.iter().collect(),
            Value::Object(values) => values.values().collect(),
            _ => return None,
        };
        let result = items
            .into_iter()
            .map(|item| read(item, rest).unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        return Some(if rest.contains(&"*") {
            collapse(result)
        } else {
            Value::Array(result)
        });
    }
    let child = match part {
        "\\*" => direct(value, "*"),
        "\\{first}" => direct(value, "{first}"),
        "\\{last}" => direct(value, "{last}"),
        "{first}" => match value {
            Value::Array(values) => values.first(),
            Value::Object(values) => values.values().next(),
            _ => None,
        },
        "{last}" => match value {
            Value::Array(values) => values.last(),
            Value::Object(values) => values.values().next_back(),
            _ => None,
        },
        _ => direct(value, part),
    }?;
    read(child, rest)
}

// Arr::collapse 使用 array_merge：整数键重新编号，字符串键后写覆盖。
fn collapse(values: Vec<Value>) -> Value {
    let mut result = Map::new();
    let mut next = 0_usize;
    for value in values {
        for (key, child) in entries(value) {
            if integer_key(&key) {
                result.insert(next.to_string(), child);
                next += 1;
            } else {
                result.insert(key, child);
            }
        }
    }
    packed(result)
}

/// 仅供输入 bag：`Arr::has` 的存在判断，null 存在且星号不展开。
pub(crate) fn has(value: &Value, key: &str) -> bool {
    if direct(value, key).is_some() {
        return true;
    }
    let mut current = value;
    for part in key.split('.') {
        let Some(child) = direct(current, part) else {
            return false;
        };
        current = child;
    }
    true
}

fn direct<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(values) => values.get(key),
        Value::Array(values) => key
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == key)
            .and_then(|index| values.get(index)),
        _ => None,
    }
}

/// 仅供输入 bag：`Arr::set` 的 dot 写入，星号为字面键；超限不修改原值。
pub(crate) fn set(target: &mut Value, key: &str, value: Value) -> crate::Result<()> {
    if key.len() > MAX_EXPANDED_KEYS || key.split('.').take(MAX_DEPTH + 1).count() > MAX_DEPTH {
        return Err(limit_error());
    }
    let parts = key.split('.').collect::<Vec<_>>();
    validate_limits(target)?;
    validate_limits(&value)?;
    let mut candidate = target.clone();
    write(&mut candidate, &parts, value);
    validate_limits(&candidate)?;
    *target = candidate;
    Ok(())
}

fn write(target: &mut Value, parts: &[&str], value: Value) {
    let Some((&key, rest)) = parts.split_first() else {
        return;
    };
    let mut values = entries(std::mem::take(target));
    if rest.is_empty() {
        values.insert(key.to_owned(), value);
    } else {
        let child = values
            .entry(key.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()));
        write(child, rest, value);
    }
    *target = packed(values);
}

/// 仅供输入 bag：`Arr::forget` 优先删除完整键，数组删除不重编号。
pub(crate) fn forget(target: &mut Value, key: &str) -> crate::Result<()> {
    validate_limits(target)?;
    if key.len() > MAX_EXPANDED_KEYS || key.split('.').take(MAX_DEPTH + 1).count() > MAX_DEPTH {
        return Err(limit_error());
    }
    if direct(target, key).is_some() {
        remove(target, key);
    } else {
        erase(target, &key.split('.').collect::<Vec<_>>());
    }
    Ok(())
}

fn erase(target: &mut Value, parts: &[&str]) {
    let Some((&key, rest)) = parts.split_first() else {
        return;
    };
    if rest.is_empty() {
        remove(target, key);
    } else {
        match target {
            Value::Object(values) => {
                if let Some(child) = values.get_mut(key) {
                    erase(child, rest);
                }
            }
            Value::Array(values) => {
                if let Ok(index) = key.parse::<usize>()
                    && index.to_string() == key
                    && let Some(child) = values.get_mut(index)
                {
                    erase(child, rest);
                }
            }
            _ => {}
        }
    }
}

fn remove(target: &mut Value, key: &str) {
    if direct(target, key).is_none() {
        return;
    }
    let mut values = entries(std::mem::take(target));
    values.shift_remove(key);
    *target = packed(values);
}

/// 仅供输入 bag：PHP 顶层左并集，显式 null 遮住 query，不递归合并。
pub(crate) fn union(source: &Value, query: &Value) -> Value {
    let mut result = entries(source.clone());
    for (key, value) in entries(query.clone()) {
        result.entry(key).or_insert(value);
    }
    packed(result)
}

/// 仅供输入 bag：JSON 的 PHP `(array)` 顶层转换及嵌套关联数组表示。
pub(crate) fn cast_json(value: Value) -> Value {
    match value {
        Value::Null => Value::Array(Vec::new()),
        Value::Array(_) | Value::Object(_) => normalize(value),
        scalar => Value::Array(vec![scalar]),
    }
}

fn normalize(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(normalize).collect()),
        Value::Object(values) => packed(
            values
                .into_iter()
                .map(|(key, value)| (key, normalize(value)))
                .collect(),
        ),
        scalar => scalar,
    }
}

fn entries(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(values) => values,
        Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        _ => Map::new(),
    }
}

fn packed(values: Map<String, Value>) -> Value {
    if values
        .keys()
        .enumerate()
        .all(|(index, key)| key == &index.to_string())
    {
        Value::Array(values.into_values().collect())
    } else {
        Value::Object(values)
    }
}

fn integer_key(key: &str) -> bool {
    key.parse::<i64>()
        .is_ok_and(|number| number.to_string() == key)
}

/// 仅供输入 bag：限制层级、全部字段及完整展开键累计字节，不回显输入。
pub(crate) fn validate_limits(value: &Value) -> crate::Result<()> {
    let mut fields = 0_usize;
    let mut expanded = 0_usize;
    visit(value, 0, 0, &mut fields, &mut expanded)
}

fn visit(
    value: &Value,
    depth: usize,
    prefix: usize,
    fields: &mut usize,
    expanded: &mut usize,
) -> crate::Result<()> {
    if depth > MAX_DEPTH {
        return Err(limit_error());
    }
    match value {
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                visit_entry(
                    child,
                    index.to_string().len(),
                    depth,
                    prefix,
                    fields,
                    expanded,
                )?;
            }
        }
        Value::Object(values) => {
            for (key, child) in values {
                visit_entry(child, key.len(), depth, prefix, fields, expanded)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn visit_entry(
    child: &Value,
    key_len: usize,
    depth: usize,
    prefix: usize,
    fields: &mut usize,
    expanded: &mut usize,
) -> crate::Result<()> {
    let path_len = prefix
        .saturating_add(usize::from(depth > 0))
        .saturating_add(key_len);
    *fields = fields.saturating_add(1);
    *expanded = expanded.saturating_add(path_len);
    if *fields > MAX_FIELDS || *expanded > MAX_EXPANDED_KEYS {
        return Err(limit_error());
    }
    visit(child, depth + 1, path_len, fields, expanded)
}

pub(crate) fn limit_error() -> crate::Error {
    crate::Error::custom_with_status(
        http::StatusCode::PAYLOAD_TOO_LARGE,
        "input_limits_exceeded",
        "请求输入超过资源限制",
    )
}

/// 按受限路径读取，并校验通配符放大后的返回值。
pub(crate) fn checked_get(bag: &Value, key: &str) -> crate::Result<Option<Value>> {
    if key.len() > MAX_EXPANDED_KEYS || key.split('.').take(MAX_DEPTH + 1).count() > MAX_DEPTH {
        return Err(limit_error());
    }
    let result = get(bag, key);
    if let Some(value) = &result {
        validate_limits(value)?;
    }
    Ok(result)
}

pub(crate) fn read_scalar<T>(
    bag: &Value,
    key: &str,
    cast: impl FnOnce(&Value) -> T,
    default: impl FnOnce() -> crate::Result<T>,
) -> crate::Result<T> {
    match checked_get(bag, key)? {
        Some(value) => Ok(cast(&value)),
        None => default(),
    }
}

/// 在同一个数组引擎中选择属性；缺失路径不写入结果。
pub(crate) fn only(bag: &Value, keys: &[&str]) -> crate::Result<Value> {
    let mut result = Value::Array(Vec::new());
    for key in keys {
        if let Some(child) = checked_get(bag, key)? {
            set(&mut result, key, child)?;
        }
    }
    validate_limits(&result)?;
    Ok(result)
}

/// Session `start` 的 `array_replace`；完整顶层键覆盖，不展开 dot 或重排旧键。
#[cfg(feature = "session")]
pub(crate) fn replace_root(current: Value, incoming: Value) -> crate::Result<Value> {
    validate_limits(&current)?;
    validate_limits(&incoming)?;
    let mut values = entries(current);
    for (key, value) in entries(incoming) {
        values.insert(key, value);
    }
    let result = packed(values);
    validate_limits(&result)?;
    Ok(result)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "单元测试的预期构造失败应立即中止用例，不用于生产路径"
)]
mod tests {
    // 固定语义来源：Laravel v13.39.0 的 Arr 与 data_get。
    // https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Collections/Arr.php
    // https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Collections/helpers.php
    use super::*;
    use serde_json::json;

    #[test]
    fn private_get_bounds_path_before_collecting_segments() {
        // 资源保护是 ADR-079 的 Rust 适配；公开接口负责 413，私有读取安全返回缺失。
        let empty = json!({});
        assert_eq!(get(&empty, &".".repeat(100_000)), None);
        assert_eq!(get(&empty, &"s".repeat(MAX_EXPANDED_KEYS + 1)), None);
        let mut bag = json!("leaf");
        for _ in 0..MAX_DEPTH {
            bag = json!({"a": bag});
        }
        let valid = std::iter::repeat_n("a", MAX_DEPTH)
            .collect::<Vec<_>>()
            .join(".");
        assert_eq!(get(&bag, &valid), Some(json!("leaf")));
        assert_eq!(get(&bag, &format!("{valid}.a")), None);
    }

    #[test]
    fn data_get_uses_segments_but_has_and_forget_prefer_exact_keys() {
        let mut bag = json!({"a.b": 1, "a": {"b": null}, "*": 2});
        assert_eq!(get(&bag, "a.b"), Some(Value::Null));
        assert!(has(&bag, "a.b"));
        assert_eq!(get(&bag, "\\*"), Some(json!(2)));
        forget(&mut bag, "a.b").unwrap();
        assert_eq!(bag, json!({"a": {"b": null}, "*": 2}));
        assert!(!has(&bag, "a.*"));
        assert_eq!(get(&bag, "a.*"), Some(json!([null])));
    }

    #[test]
    fn wildcard_keeps_missing_null_and_collapse_only_when_more_wildcards_remain() {
        let bag = json!({"groups": [
            {"users": [{"name": "A"}, {}, {"name": null}]},
            {"users": [{"name": "B"}]},
            {}
        ]});
        assert_eq!(
            get(&bag, "groups.*.users.*.name"),
            Some(json!(["A", null, null, "B"]))
        );
        assert_eq!(
            get(&bag, "groups.*.missing"),
            Some(json!([null, null, null]))
        );
        assert_eq!(get(&json!(4), "*"), None);
        assert_eq!(get(&json!([]), "*"), Some(json!([])));
    }

    #[test]
    fn nested_wildcard_collapse_reindexes_numeric_and_overwrites_string_keys() {
        let bag = json!([
            {"items": [{"x": 1, "2": "first"}]},
            {"items": [{"x": 2, "8": "second"}]}
        ]);
        // 剩余 * 触发 Arr::collapse，但最后一级读取本身保留各 item 的数组。
        assert_eq!(
            get(&bag, "*.items.*"),
            Some(json!([
                {"x": 1, "2": "first"}, {"x": 2, "8": "second"}
            ]))
        );
        assert_eq!(
            collapse(vec![
                json!({"x": 1, "-1": "a"}),
                json!({"x": 2, "00": "s", "9": "b"})
            ]),
            json!({"x": 2, "0": "a", "00": "s", "1": "b"})
        );
    }

    #[test]
    fn first_last_and_escaped_special_segments_follow_data_get() {
        let bag = json!({"z": 10, "a": 20, "{first}": 30, "{last}": 40});
        assert_eq!(get(&bag, "{first}"), Some(json!(10)));
        assert_eq!(get(&bag, "{last}"), Some(json!(40)));
        assert_eq!(get(&bag, "\\{first}"), Some(json!(30)));
        assert_eq!(get(&bag, "\\{last}"), Some(json!(40)));
    }

    #[test]
    fn writing_replaces_scalar_and_keeps_literal_star_and_empty_segments() {
        let mut bag = json!({"user": 3, "a.b": 9});
        set(&mut bag, "user.*.name", json!("N")).unwrap();
        set(&mut bag, "a.b", json!(4)).unwrap();
        set(&mut bag, ".", json!(true)).unwrap();
        assert_eq!(
            bag,
            json!({"user": {"*": {"name": "N"}}, "a.b": 9, "a": {"b": 4}, "": {"": true}})
        );
        assert!(has(&bag, "user.*.name"));
    }

    #[test]
    fn numeric_keys_pack_only_in_order_and_sparse_deletion_keeps_original_indices() {
        let mut bag = json!([]);
        set(&mut bag, "0", json!("a")).unwrap();
        set(&mut bag, "1", json!("b")).unwrap();
        assert_eq!(bag, json!(["a", "b"]));
        assert_eq!(get(&bag, "00"), None);
        forget(&mut bag, "0").unwrap();
        assert_eq!(bag, json!({"1": "b"}));
        set(&mut bag, "-1", json!("negative")).unwrap();
        set(&mut bag, "00", json!("leading")).unwrap();
        assert_eq!(get(&bag, "-1"), Some(json!("negative")));
        assert_eq!(get(&bag, "00"), Some(json!("leading")));
        set(&mut bag, "0", json!("new")).unwrap();
        assert!(bag.is_object());
        assert_eq!(
            bag.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["1", "-1", "00", "0"]
        );
    }

    #[test]
    fn null_and_top_level_left_union_do_not_fall_through_or_merge_deeply() {
        let result = union(
            &json!({"x": null, "user": {"name": "A"}}),
            &json!({"x": 5, "user": {"age": 3}, "y": 6}),
        );
        assert_eq!(result, json!({"x": null, "user": {"name": "A"}, "y": 6}));
        assert!(has(&result, "x"));
        assert!(!has(&result, "user.age"));
        assert_eq!(union(&json!([1]), &json!([2, 3])), json!([1, 3]));
    }

    #[test]
    fn json_cast_wraps_scalars_but_preserves_nested_null_and_php_numeric_identity() {
        assert_eq!(cast_json(Value::Null), json!([]));
        assert_eq!(cast_json(json!(false)), json!([false]));
        assert_eq!(cast_json(json!("s")), json!(["s"]));
        assert_eq!(
            cast_json(json!({"0": {"0": null, "1": 2}, "1": 3})),
            json!([[null, 2], 3])
        );
        assert!(cast_json(json!({"00": 1, "-1": 2})).is_object());
        assert!(cast_json(json!({"1": 1, "0": 2})).is_object());
    }

    #[test]
    fn limits_count_container_entries_and_reject_before_mutating() {
        let mut bag = Value::Array(vec![Value::Null; MAX_FIELDS]);
        validate_limits(&bag).unwrap();
        let before = bag.clone();
        let error = set(&mut bag, "1024", json!("private input")).unwrap_err();
        assert_eq!(error.status(), http::StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.code(), "input_limits_exceeded");
        assert!(!error.to_string().contains("private"));
        assert_eq!(bag, before);
        assert!(
            validate_limits(&json!({"a": Value::Array(vec![Value::Null; MAX_FIELDS])})).is_err()
        );
    }

    #[test]
    fn limits_allow_64_children_and_reject_65_and_expanded_prefix_amplification() {
        let mut bag = Value::Null;
        for _ in 0..MAX_DEPTH {
            bag = json!({"a": bag});
        }
        validate_limits(&bag).unwrap();
        assert!(validate_limits(&json!({"a": bag})).is_err());
        let mut object = Map::new();
        object.insert(
            "sensitive".repeat(2400),
            Value::Array(vec![Value::Null; 1000]),
        );
        assert!(validate_limits(&Value::Object(object)).is_err());
    }
}

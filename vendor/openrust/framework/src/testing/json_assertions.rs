use serde_json::{Map, Value};

use super::TestResponse;
use crate::support::{Arr, data_get, php_loose_value_equal};

impl TestResponse {
    fn json_value(&self) -> Value {
        self.json().expect("响应不是有效 JSON")
    }

    #[track_caller]
    pub fn assert_json(&self, expected: &Value, strict: bool) -> &Self {
        let actual = self.json_value();
        assert!(
            actual.is_array() || actual.is_object(),
            "JSON 子集要求数组或对象"
        );
        assert!(
            expected.is_array() || expected.is_object(),
            "JSON 子集必须为数组或对象"
        );
        let replaced = replace_recursive(&actual, expected);
        assert!(
            if strict {
                strict_equal(&actual, &replaced)
            } else {
                php_loose_value_equal(&actual, &replaced)
            },
            "JSON 不包含预期子集 {expected}，实际为 {actual}"
        );
        self
    }

    #[track_caller]
    pub fn assert_json_count(&self, count: usize, key: Option<&str>) -> &Self {
        let value = data_get(&self.json_value(), key).unwrap_or(Value::Null);
        let actual = match &value {
            Value::Array(items) => items.len(),
            Value::Object(items) => items.len(),
            _ => panic!("JSON 路径 {key:?} 不是可计数的数组或对象"),
        };
        assert_eq!(actual, count, "JSON 项目数量不匹配");
        self
    }

    #[track_caller]
    pub fn assert_json_paths(&self, paths: &Map<String, Value>) -> &Self {
        for (path, value) in paths {
            self.assert_json_path(path, value);
        }
        self
    }

    #[track_caller]
    pub fn assert_json_path_canonicalizing(&self, path: &str, expected: &Value) -> &Self {
        let actual = data_get(&self.json_value(), Some(path)).unwrap_or(Value::Null);
        assert!(
            canonical_equal(&actual, expected),
            "JSON 路径 {path} 的内容不匹配，实际 {actual}，预期 {expected}"
        );
        self
    }

    #[track_caller]
    pub fn assert_json_paths_canonicalizing(&self, paths: &Map<String, Value>) -> &Self {
        for (path, value) in paths {
            self.assert_json_path_canonicalizing(path, value);
        }
        self
    }

    #[track_caller]
    pub fn assert_json_fragment(&self, expected: &Value) -> &Self {
        let found = self.fragment_matches(expected);
        assert!(found.iter().all(|found| *found), "JSON 缺少片段 {expected}");
        self
    }

    #[track_caller]
    pub fn assert_json_fragments(&self, expected: &[Value]) -> &Self {
        for fragment in expected {
            self.assert_json_fragment(fragment);
        }
        self
    }

    #[track_caller]
    pub fn assert_json_missing(&self, unexpected: &Value, exact: bool) -> &Self {
        let found = self.fragment_matches(unexpected);
        assert!(
            if exact {
                found.iter().any(|found| !found)
            } else {
                found.iter().all(|found| !found)
            },
            "JSON 包含不应出现的片段 {unexpected}"
        );
        self
    }

    #[track_caller]
    pub fn assert_json_missing_exact(&self, unexpected: &Value) -> &Self {
        self.assert_json_missing(unexpected, true)
    }

    fn fragment_matches(&self, expected: &Value) -> Vec<bool> {
        let actual = sorted(&self.json_value());
        let actual = encode(&actual);
        entries(&sorted(expected))
            .into_iter()
            .map(|(key, value)| {
                let pair = if key == "0" {
                    Value::Array(vec![value])
                } else {
                    Value::Object(Map::from_iter([(key, value)]))
                };
                let pair = encode(&pair);
                let needle = &pair[1..pair.len() - 1];
                [']', '}', ',']
                    .iter()
                    .any(|suffix| actual.contains(&format!("{needle}{suffix}")))
            })
            .collect()
    }

    #[track_caller]
    pub fn assert_json_missing_path(&self, path: &str) -> &Self {
        let actual = self.json_value();
        let exists = if path.contains('*') {
            let pattern = path
                .split('.')
                .map(|part| {
                    if part == "*" {
                        "[^.]+".to_owned()
                    } else {
                        regex::escape(part)
                    }
                })
                .collect::<Vec<_>>()
                .join("\\.");
            let pattern =
                regex::Regex::new(&format!("^{pattern}(\\.|$)")).expect("JSON 路径正则表达式");
            Arr::dot(&actual)
                .expect("JSON 深度超过支持范围")
                .keys()
                .any(|key| pattern.is_match(key))
        } else {
            Arr::has(&actual, path)
        };
        assert!(!exists, "JSON 包含不应出现的路径 {path}");
        self
    }

    #[track_caller]
    pub fn assert_json_missing_paths(&self, paths: &[&str]) -> &Self {
        for path in paths {
            self.assert_json_missing_path(path);
        }
        self
    }

    #[track_caller]
    pub fn assert_exact_json_structure(&self, structure: &Value) -> &Self {
        check_structure(&self.json_value(), structure, true);
        self
    }

    #[track_caller]
    pub fn assert_similar_json(&self, expected: &Value) -> &Self {
        assert_eq!(
            encode(&sorted(&self.json_value())),
            encode(&sorted(expected)),
            "JSON 内容不相似"
        );
        self
    }

    #[track_caller]
    pub fn assert_json_is_array(&self, key: Option<&str>) -> &Self {
        let value = data_get(&self.json_value(), key).unwrap_or(Value::Null);
        assert!(is_php_list(&value), "JSON 路径 {key:?} 不是列表");
        self
    }

    #[track_caller]
    pub fn assert_json_is_object(&self, key: Option<&str>) -> &Self {
        let value = data_get(&self.json_value(), key).unwrap_or(Value::Null);
        assert!(
            value.is_object() && !is_php_list(&value),
            "JSON 路径 {key:?} 不是关联对象"
        );
        self
    }
}

pub(super) fn strict_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(_) | Value::Object(_), Value::Array(_) | Value::Object(_)) => {
            let left = entries(left);
            let right = entries(right);
            left.len() == right.len()
                && left
                    .iter()
                    .zip(&right)
                    .all(|((lk, lv), (rk, rv))| lk == rk && strict_equal(lv, rv))
        }
        _ => left == right,
    }
}

fn entries(value: &Value) -> Vec<(String, Value)> {
    match value {
        Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect(),
        Value::Object(values) => values.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => Vec::new(),
    }
}

fn replace_recursive(actual: &Value, expected: &Value) -> Value {
    let mut result: Map<_, _> = entries(actual).into_iter().collect();
    for (key, value) in entries(expected) {
        let value = match result.get(&key) {
            Some(current)
                if (current.is_array() || current.is_object())
                    && (value.is_array() || value.is_object()) =>
            {
                replace_recursive(current, &value)
            }
            _ => value,
        };
        result.insert(key, value);
    }
    Value::Object(result)
}

fn canonical_equal(left: &Value, right: &Value) -> bool {
    if (left.is_array() || left.is_object()) && (right.is_array() || right.is_object()) {
        let mut left_entries = entries(left);
        let mut right_entries = entries(right);
        if left_entries.len() != right_entries.len() {
            return false;
        }
        if is_php_list(left) && is_php_list(right) {
            let rank = |value: &Value| match value {
                Value::Null => 0,
                Value::Bool(_) => 1,
                Value::Number(_) => 2,
                Value::String(_) => 3,
                _ => 4,
            };
            let order = |(_, left): &(String, Value), (_, right): &(String, Value)| {
                rank(left).cmp(&rank(right)).then_with(|| {
                    if left.is_array() || left.is_object() {
                        serialized(left).cmp(&serialized(right))
                    } else {
                        scalar_order(left, right)
                    }
                })
            };
            left_entries.sort_by(order);
            right_entries.sort_by(order);
            left_entries
                .iter()
                .zip(&right_entries)
                .all(|((_, left), (_, right))| canonical_equal(left, right))
        } else {
            left_entries.iter().all(|(key, value)| {
                right_entries
                    .iter()
                    .any(|(other, item)| key == other && canonical_equal(value, item))
            })
        }
    } else if left.is_array() || left.is_object() || right.is_array() || right.is_object() {
        false
    } else if (left.is_string() || right.is_string()) && !left.is_boolean() && !right.is_boolean() {
        crate::support::input_scalars::string(left.clone())
            == crate::support::input_scalars::string(right.clone())
    } else {
        php_loose_value_equal(left, right)
    }
}

fn scalar_order(left: &Value, right: &Value) -> std::cmp::Ordering {
    if php_loose_value_equal(left, right) {
        return std::cmp::Ordering::Equal;
    }
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => match (left.as_i64(), right.as_i64()) {
            (Some(left), Some(right)) => left.cmp(&right),
            _ => left
                .as_f64()
                .expect("JSON 数值")
                .total_cmp(&right.as_f64().expect("JSON 数值")),
        },
        (Value::String(left), Value::String(right)) => {
            match (
                crate::support::input_scalars::decimal_float(left),
                crate::support::input_scalars::decimal_float(right),
            ) {
                (Some(left), Some(right)) => left.total_cmp(&right),
                _ => left.cmp(right),
            }
        }
        _ => crate::support::input_scalars::string(left.clone())
            .cmp(&crate::support::input_scalars::string(right.clone())),
    }
}

fn serialized(value: &Value) -> String {
    match value {
        Value::Null => "N;".into(),
        Value::Bool(value) => format!("b:{};", u8::from(*value)),
        Value::Number(value) => format!(
            "{}:{};",
            if value.is_f64() { "d" } else { "i" },
            encode(&Value::Number(value.clone()))
        ),
        Value::String(value) => format!("s:{}:\"{}\";", value.len(), value),
        _ => {
            let items = entries(value);
            let content = items
                .iter()
                .map(|(key, value)| {
                    let key = key
                        .parse::<i64>()
                        .ok()
                        .filter(|number| number.to_string() == *key)
                        .map_or_else(|| Value::String(key.clone()), Value::from);
                    format!("{}{}", serialized(&key), serialized(value))
                })
                .collect::<String>();
            format!("a:{}:{{{content}}}", items.len())
        }
    }
}

fn is_php_list(value: &Value) -> bool {
    match value {
        Value::Array(_) => true,
        Value::Object(items) => items
            .keys()
            .enumerate()
            .all(|(index, key)| key == &index.to_string()),
        _ => false,
    }
}

fn sorted(value: &Value) -> Value {
    Arr::sortRecursive(value, false).expect("JSON 深度超过支持范围")
}

pub(super) fn encode(value: &Value) -> String {
    match value {
        Value::Number(number) => {
            crate::support::php_numeric_json_string(&number.to_string(), false)
                .expect("JSON 数值必须有限")
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(encode).collect::<Vec<_>>().join(",")
        ),
        Value::Object(items) if is_php_list(value) => format!(
            "[{}]",
            items.values().map(encode).collect::<Vec<_>>().join(",")
        ),
        Value::Object(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(k, v)| format!("{}:{}", encode(&Value::String(k.clone())), encode(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => serde_json::to_string(value)
            .expect("有效 JSON")
            .replace('/', "\\/")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029"),
    }
}

pub(super) fn check_structure(data: &Value, shape: &Value, exact: bool) {
    let fields = entries(shape);
    assert!(
        shape.is_array() || shape.is_object(),
        "JSON 结构必须为字段列表或嵌套对象"
    );
    let keys: Vec<_> = fields
        .iter()
        .map(|(key, value)| {
            if value.is_array() || value.is_object() {
                key.clone()
            } else {
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned)
            }
        })
        .collect();
    if exact {
        assert!(
            data.is_array() || data.is_object(),
            "严格 JSON 结构要求数组或对象"
        );
        if keys != ["*"] {
            let mut actual: Vec<_> = entries(data).into_iter().map(|(key, _)| key).collect();
            let mut keys = keys.clone();
            actual.sort();
            keys.sort();
            assert_eq!(actual, keys, "JSON 结构字段不匹配");
        }
    }
    for (key, value) in fields {
        if value.is_array() || value.is_object() {
            if key == "*" {
                assert!(
                    data.is_array() || data.is_object(),
                    "JSON 通配结构要求数组或对象"
                );
                for (_, item) in entries(data) {
                    check_structure(&item, &value, exact);
                }
            } else {
                let child = direct(data, &key).unwrap_or_else(|| panic!("JSON 缺少字段 {key}"));
                // PHP 的 responseData=null 参数会回到当前响应，而不是构造新节点。
                check_structure(if child.is_null() { data } else { child }, &value, exact);
            }
        } else {
            let key = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned);
            assert!(direct(data, &key).is_some(), "JSON 缺少字段 {key}");
        }
    }
}

fn direct<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(items) => items.get(key),
        Value::Array(items) => key
            .parse::<usize>()
            .ok()
            .filter(|i| i.to_string() == key)
            .and_then(|i| items.get(i)),
        _ => None,
    }
}

pub(super) fn exact_json(actual: &Value, expected: &Value) -> bool {
    fn reorder(value: &Value) -> Value {
        let fields = Arr::dot(value).expect("JSON 深度超过支持范围");
        let mut fields: Vec<_> = fields.into_iter().collect();
        fields.sort_by(|(left, _), (right, _)| left.cmp(right));
        let mut result = Value::Array(Vec::new());
        for (key, value) in fields {
            Arr::set(&mut result, &key, value).expect("JSON 路径超过支持范围");
        }
        result
    }
    encode(&reorder(actual)) == encode(&reorder(expected))
}

pub(super) fn phpunit_equal(left: &Value, right: &Value) -> bool {
    if (left.is_array() || left.is_object()) && (right.is_array() || right.is_object()) {
        let left = entries(left);
        let right = entries(right);
        left.len() == right.len()
            && left.iter().all(|(key, value)| {
                right
                    .iter()
                    .any(|(other, item)| key == other && phpunit_equal(value, item))
            })
    } else if left.is_array() || left.is_object() || right.is_array() || right.is_object() {
        false
    } else if (left.is_string() || right.is_string()) && !left.is_boolean() && !right.is_boolean() {
        crate::support::input_scalars::string(left.clone())
            == crate::support::input_scalars::string(right.clone())
    } else {
        php_loose_value_equal(left, right)
    }
}

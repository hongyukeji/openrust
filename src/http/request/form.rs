//! PHP 表单变量注册：解码与有序键树分离，最后才投影为 JSON 值。

use axum::http::StatusCode;
use serde_json::{Map, Value};

use crate::core::Error;

const MAX_FIELDS: usize = 1024;
const MAX_DEPTH: usize = 64;

/// 按 PHP `GET/parse_str` 的 C 字符串边界解析查询输入。
pub(super) fn parse_query(raw: &str) -> crate::core::Result<Value> {
    parse(raw, Mode::Query)
}

/// 按 PHP POST 的长度边界解析表单，保留值中的原始 NUL 及后续字段。
pub(super) fn parse_form(raw: &str) -> crate::core::Result<Value> {
    parse(raw, Mode::Form)
}

#[derive(Clone, Copy)]
enum Mode {
    Query,
    Form,
}

fn parse(raw: &str, mode: Mode) -> crate::core::Result<Value> {
    let mut root = Bag::default();
    // GET 使用 strtok/C 字符串；POST 的 add_post_var 使用 memchr 与显式长度。
    let raw = match mode {
        Mode::Query => raw.split('\0').next().unwrap_or_default(),
        Mode::Form => raw,
    };
    for (count, pair) in raw.split('&').filter(|pair| !pair.is_empty()).enumerate() {
        if count >= MAX_FIELDS {
            return Err(limits_error());
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        validate_encoding(key)?;
        validate_encoding(value)?;
        let Some((key, value)) = url::form_urlencoded::parse(pair.as_bytes()).next() else {
            continue;
        };
        if let Some(path) = path(&key)? {
            root.insert(&path, value.into_owned(), &key);
        }
    }
    let value = root.into_value();
    super::value::validate_limits(&value)?;
    Ok(value)
}

fn limits_error() -> Error {
    Error::custom_with_status(
        StatusCode::PAYLOAD_TOO_LARGE,
        "input_limits_exceeded",
        "请求输入超过资源限制",
    )
}

// PHP 字符串容纳任意字节；Rust Value 仅容纳 UTF-8，拒绝代替静默丢失字节身份。
fn validate_encoding(raw: &str) -> crate::core::Result<()> {
    let mut decoded = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] == b'%'
            && position + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[position + 1]), hex(bytes[position + 2]))
        {
            decoded.push(high * 16 + low);
            position += 3;
            continue;
        }
        decoded.push(if bytes[position] == b'+' {
            b' '
        } else {
            bytes[position]
        });
        position += 1;
    }
    std::str::from_utf8(&decoded).map(|_| ()).map_err(|_| {
        Error::custom_with_status(
            StatusCode::BAD_REQUEST,
            "input_encoding_invalid",
            "请求输入必须使用有效 UTF-8",
        )
    })
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, PartialEq)]
enum Key {
    Integer(i64),
    String(String),
}

impl Key {
    fn named(name: &str) -> Self {
        // Zend 数字键不接受 +、前导零或 -0；超出 64 位整数范围则仍是字符串。
        if let Ok(integer) = name.parse::<i64>()
            && integer.to_string() == name
        {
            return Self::Integer(integer);
        }
        Self::String(name.to_owned())
    }

    fn into_string(self) -> String {
        match self {
            Self::Integer(integer) => integer.to_string(),
            Self::String(string) => string,
        }
    }
}

fn forbidden(name: &str, original: &str) -> bool {
    ["__Host-", "__Secure-"]
        .into_iter()
        .any(|prefix| name.starts_with(prefix) && !original.starts_with(prefix))
}

fn normalize(name: &str) -> String {
    name.replace([' ', '.', '['], "_")
}

fn path(name: &str) -> crate::core::Result<Option<Vec<Option<Key>>>> {
    let name = name
        .split('\0')
        .next()
        .unwrap_or_default()
        .trim_start_matches(' ');
    let first = name.find('[').unwrap_or(name.len());
    let top = normalize(&name[..first]);
    if top.is_empty() || forbidden(&top, name) {
        return Ok(None);
    }
    let mut path = vec![Some(Key::named(&top))];
    let mut rest = &name[first..];
    while let Some(after_open) = rest.strip_prefix('[') {
        if path.len() > MAX_DEPTH {
            return Err(limits_error());
        }
        let Some(end) = after_open.find(']') else {
            // 首层缺右括号时 PHP 将原变量名扁平化；深层则保留当前父键。
            if path.len() == 1 {
                path[0] = Some(Key::named(&normalize(name)));
            }
            break;
        };
        let index = &after_open[..end];
        let append = index.is_empty()
            || (index.len() == 1
                && (index.as_bytes()[0].is_ascii_whitespace() || index.as_bytes()[0] == 0x0b));
        path.push(if append {
            None
        } else {
            Some(Key::named(index))
        });
        rest = &after_open[end + 1..];
        if !rest.starts_with('[') {
            break;
        }
    }
    Ok(Some(path))
}

#[derive(Default)]
struct Bag {
    entries: Vec<(Key, Node)>,
    next: Option<i64>,
}

enum Node {
    Scalar(String),
    Array(Bag),
}

impl Bag {
    fn insert(&mut self, path: &[Option<Key>], value: String, original: &str) {
        let Some((head, tail)) = path.split_first() else {
            return;
        };
        let key = match head {
            Some(Key::String(name)) if forbidden(name, original.trim_start_matches(' ')) => return,
            Some(key) => key.clone(),
            None => {
                let next = self.next.unwrap_or(0);
                // LONG_MAX 后的 append 在 PHP 中失败，不覆盖同名的已存在项。
                if self
                    .entries
                    .iter()
                    .any(|(key, _)| *key == Key::Integer(next))
                {
                    return;
                }
                Key::Integer(next)
            }
        };
        if let Key::Integer(integer) = &key
            && self.next.is_none_or(|next| *integer >= next)
        {
            self.next = Some(integer.saturating_add(1));
        }
        let position = self
            .entries
            .iter()
            .position(|(existing, _)| *existing == key);
        let position = if let Some(position) = position {
            position
        } else {
            self.entries.push((key, Node::Array(Self::default())));
            self.entries.len() - 1
        };
        if tail.is_empty() {
            self.entries[position].1 = Node::Scalar(value);
        } else {
            if matches!(self.entries[position].1, Node::Scalar(_)) {
                self.entries[position].1 = Node::Array(Self::default());
            }
            if let Node::Array(bag) = &mut self.entries[position].1 {
                bag.insert(tail, value, original);
            }
        }
    }

    fn into_value(self) -> Value {
        // 同时检查顺序，避免把 PHP 的 [1 => a, 0 => b] 重排而丢失观察语义。
        let sequential = self.entries.iter().enumerate().all(|(index, (key, _))| {
            matches!(key, Key::Integer(integer) if usize::try_from(*integer) == Ok(index))
        });
        if sequential {
            Value::Array(
                self.entries
                    .into_iter()
                    .map(|(_, node)| node.into_value())
                    .collect(),
            )
        } else {
            Value::Object(
                self.entries
                    .into_iter()
                    .map(|(key, node)| (key.into_string(), node.into_value()))
                    .collect::<Map<_, _>>(),
            )
        }
    }
}

impl Node {
    fn into_value(self) -> Value {
        match self {
            Self::Scalar(value) => Value::String(value),
            Self::Array(bag) => bag.into_value(),
        }
    }
}

#[cfg(test)]
// 测试断言解析成功；生产代码不使用 expect。
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parsed(raw: &str) -> Value {
        parse_form(raw).expect("表单应当成功解析")
    }

    #[test]
    fn repeated_and_nested_variables_follow_php_overwrite_order() {
        assert_eq!(parsed("a=1&a=2"), json!({"a":"2"}));
        assert_eq!(parsed("a[]=1&a[]=2"), json!({"a":["1","2"]}));
        assert_eq!(parsed("a[b]=1&a[b]=2"), json!({"a":{"b":"2"}}));
        assert_eq!(parsed("a=1&a[b]=2"), json!({"a":{"b":"2"}}));
        assert_eq!(parsed("a[b]=1&a=2"), json!({"a":"2"}));
        assert_eq!(
            parsed("a[][x]=1&a[][y]=2"),
            json!({"a":[{"x":"1"},{"y":"2"}]})
        );
        assert_eq!(
            parsed("a[0]=first&a[]=second&a[0]=last"),
            json!({"a":["last","second"]})
        );
    }

    #[test]
    fn numeric_keys_preserve_sparse_mixed_negative_and_insertion_semantics() {
        assert_eq!(parsed("a[0]=x&a[2]=z"), json!({"a":{"0":"x","2":"z"}}));
        assert_eq!(parsed("a[-2]=x&a[]=y"), json!({"a":{"-2":"x","-1":"y"}}));
        assert_eq!(
            parsed("a[01]=x&a[+1]=y&a[-0]=z&a[]=w"),
            json!({"a":{"01":"x"," 1":"y","-0":"z","0":"w"}})
        );
        assert_eq!(
            parsed("a[%2B1]=x&a[9223372036854775808]=y"),
            json!({"a":{"+1":"x","9223372036854775808":"y"}})
        );
        assert_eq!(
            parsed("a[9223372036854775807]=x&a[]=ignored"),
            json!({"a":{"9223372036854775807":"x"}})
        );
        let reversed = parsed("a[1]=x&a[0]=y&a[label]=z");
        assert_eq!(
            reversed["a"]
                .as_object()
                .map(|map| map.keys().cloned().collect::<Vec<_>>()),
            Some(vec!["1".into(), "0".into(), "label".into()])
        );
    }

    #[test]
    fn names_and_malformed_brackets_follow_variable_registration() {
        assert_eq!(
            parsed("+a.b+c=1&a[b.c+d]=2"),
            json!({"a_b_c":"1","a":{"b.c d":"2"}})
        );
        assert_eq!(
            parsed("=1&[]=2&++=3&&quote%22=value%22"),
            json!({"quote\"":"value\""})
        );
        assert_eq!(
            parsed("a[b=1&a[c][oops=2"),
            json!({"a_b":"1","a":{"c":"2"}})
        );
        assert_eq!(
            parsed("a[b]suffix=1&a[+]=2&a[%09]=3&a[%0B]=4"),
            json!({"a":{"b":"1","0":"2","1":"3","2":"4"}})
        );
        assert_eq!(
            parsed("a%00ignored=x&value=nul%00kept"),
            json!({"a":"x","value":"nul\0kept"})
        );
        assert_eq!(
            parsed("a[__Host-secret]=ignored&__Host-safe=kept"),
            json!({"a":[],"__Host-safe":"kept"})
        );
    }

    #[test]
    fn url_decoding_retains_unicode_literal_percent_quotes_and_plus_semantics() {
        assert_eq!(
            parsed("城市=上海&name=hello+world&plus=%2B&bad=%xy%2&quote=%22hi%22"),
            json!({"城市":"上海","name":"hello world","plus":"+","bad":"%xy%2","quote":"\"hi\""})
        );
        assert_eq!(parsed("valid=%EF%BF%BD"), json!({"valid":"�"}));
        for raw in ["a=%FF", "%FE=x", "a=%C0%AF", "a=%E2%82"] {
            let error = parse_form(raw).expect_err("无效 UTF-8 必须拒绝");
            assert_eq!(error.status(), StatusCode::BAD_REQUEST);
            assert_eq!(error.code(), "input_encoding_invalid");
            assert!(!error.to_string().contains(raw));
        }
    }

    #[test]
    fn empty_input_is_a_php_empty_array() {
        for raw in ["", "=x", "[]=x", "&&"] {
            assert_eq!(parse_query(raw).expect("空查询应成功"), json!([]));
            assert_eq!(parsed(raw), json!([]));
        }
    }

    #[test]
    fn raw_nul_uses_distinct_get_and_post_boundaries() {
        let raw = "value=before\0after&tail=kept";
        assert_eq!(
            parse_query(raw).expect("查询应成功"),
            json!({"value":"before"})
        );
        assert_eq!(parsed(raw), json!({"value":"before\0after", "tail":"kept"}));
        let raw = "key\0ignored=value&tail=kept";
        assert_eq!(parse_query(raw).expect("查询应成功"), json!({"key":""}));
        assert_eq!(parsed(raw), json!({"key":"value", "tail":"kept"}));
        for raw in [
            "key%00ignored=value&tail=kept",
            "key=value%00kept&tail=kept",
        ] {
            let expected = if raw.starts_with("key%00") {
                json!({"key":"value", "tail":"kept"})
            } else {
                json!({"key":"value\0kept", "tail":"kept"})
            };
            assert_eq!(parse_query(raw).expect("百分号 NUL 查询应成功"), expected);
            assert_eq!(parsed(raw), expected);
        }
    }

    #[test]
    fn resource_limits_reject_without_partial_results() {
        assert!(parse_form(&vec!["a=x"; MAX_FIELDS].join("&")).is_ok());
        let error = parse_form(&vec!["a=x"; MAX_FIELDS + 1].join("&")).expect_err("字段超限");
        assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.code(), "input_limits_exceeded");
        // 根为第 0 层，标量所在容器 entry 的深度最多 64。
        assert!(parse_form(&format!("a{}=x", "[b]".repeat(MAX_DEPTH - 1))).is_ok());
        assert!(parse_form(&format!("a{}=x", "[b]".repeat(MAX_DEPTH))).is_err());
        let key = "k".repeat(1024 * 1024);
        assert!(parse_form(&format!("{key}{}=x", "[b]".repeat(21))).is_err());
    }
}

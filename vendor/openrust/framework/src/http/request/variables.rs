//! PHP-compatible variable registration for request parameters.
use crate::http::HttpError;
use serde_json::{Map, Value};
const MAX_DEPTH: usize = 64;
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

fn path(name: &str) -> Result<Option<Vec<Option<Key>>>, HttpError> {
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
            return Err(HttpError::PayloadTooLarge);
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
pub(super) struct VariableBag {
    entries: Vec<(Key, Node)>,
    next: Option<i64>,
}

enum Node {
    Scalar(String),
    Array(VariableBag),
}

impl VariableBag {
    pub(super) fn insert(&mut self, name: &str, value: String) -> Result<(), HttpError> {
        if let Some(path) = path(name)? {
            self.insert_path(&path, value, name);
        }
        Ok(())
    }

    fn insert_path(&mut self, path: &[Option<Key>], value: String, original: &str) {
        let Some((head, tail)) = path.split_first() else {
            return;
        };
        let key = match head {
            Some(Key::String(name)) if forbidden(name, original.trim_start_matches(' ')) => {
                return;
            }
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
                bag.insert_path(tail, value, original);
            }
        }
    }

    pub(super) fn into_value(self) -> Value {
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

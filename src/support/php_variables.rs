//! Query、表单及 Cookie 共用的 PHP 变量名注册树；wire 解码由各调用入口负责。
use serde_json::{Map, Value};
pub(crate) const MAX_DEPTH: usize = 64;
fn limits_error() -> crate::Error {
    crate::Error::custom_with_status(
        http::StatusCode::PAYLOAD_TOO_LARGE,
        "input_limits_exceeded",
        "请求输入超过资源限制",
    )
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
pub(crate) struct VariableBag {
    entries: Vec<(Key, Node)>,
    next: Option<i64>,
}

enum Node {
    Scalar(String),
    Array(VariableBag),
}

#[derive(Clone, Copy)]
pub(crate) enum Registration {
    Input,
    Cookie,
}
impl VariableBag {
    pub(crate) fn insert(
        &mut self,
        name: &str,
        value: String,
        mode: Registration,
    ) -> crate::Result<Option<String>> {
        let Some(path) = path(name)? else {
            return Ok(None);
        };
        let root = path.first().and_then(Option::as_ref).cloned();
        let root_name = root.clone().map(Key::into_string);
        if matches!(mode, Registration::Cookie)
            && path.len() == 1
            && root
                .as_ref()
                .is_some_and(|key| self.entries.iter().any(|(existing, _)| existing == key))
        {
            return Ok(root_name);
        }
        self.insert_path(&path, value, name);
        Ok(root_name)
    }
    fn insert_path(&mut self, path: &[Option<Key>], value: String, original: &str) {
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
                bag.insert_path(tail, value, original);
            }
        }
    }

    pub(crate) fn into_value(self) -> Value {
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

//! URI Query、请求输入及 Cookie 共用的 PHP 变量注册树；解码由各入口负责。
use serde_json::{Map, Value};
use std::borrow::Cow;
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

fn unmangled_path(name: &str) -> crate::Result<Vec<Option<Key>>> {
    let mut path = Vec::new();
    let mut name = Cow::Borrowed(name);
    loop {
        let Some((key, index, suffix)) = name.split_once('[').and_then(|(key, rest)| {
            rest.split_once(']')
                .map(|(index, suffix)| (key, index, suffix))
        }) else {
            path.push(Some(Key::named(&name)));
            return Ok(path);
        };
        if path.len() >= MAX_DEPTH {
            return Err(limits_error());
        }
        path.push(Some(Key::named(if key.is_empty() { "0" } else { key })));
        // League 将下标拼回完整后缀再解析。比如 a[][b] 的空中间键成为整数 0。
        let suffix = if suffix.starts_with('[') && suffix.contains(']') {
            suffix
        } else {
            ""
        };
        if suffix.is_empty() {
            path.push((!index.is_empty()).then(|| Key::named(index)));
            return Ok(path);
        }
        name = Cow::Owned([index, suffix].concat());
    }
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
        self.insert_path(&path, value, Some(name))?;
        Ok(root_name)
    }

    /// 插入已解码的 URI 查询参数，保留名称的原字节和 League 的括号规则。
    pub(crate) fn insert_unmangled(&mut self, name: &str, value: String) -> crate::Result<()> {
        if name.is_empty() {
            return Ok(());
        }
        let path = unmangled_path(name)?;
        self.insert_path(&path, value, None)
    }

    fn insert_path(
        &mut self,
        path: &[Option<Key>],
        value: String,
        original: Option<&str>,
    ) -> crate::Result<()> {
        let Some((head, tail)) = path.split_first() else {
            return Ok(());
        };
        let key = match head {
            Some(Key::String(name))
                if original
                    .is_some_and(|original| forbidden(name, original.trim_start_matches(' '))) =>
            {
                return Ok(());
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
                    return if original.is_some() {
                        Ok(())
                    } else {
                        Err(limits_error())
                    };
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
                bag.insert_path(tail, value, original)?;
            }
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::{MAX_DEPTH, Registration, VariableBag};
    use serde_json::json;

    #[test]
    fn unmangled_variables_keep_literal_names_and_league_nested_brackets() -> crate::Result<()> {
        let mut bag = VariableBag::default();
        for (key, value) in [
            ("", "ignored"),
            (" dot.key", "literal"),
            ("\0key", "nul"),
            ("x[broken", "literal bracket"),
            ("a[b][broken", "tail discarded"),
            ("a[][c]", "first"),
            ("a[][c]", "last"),
            ("[a]", "empty root"),
            ("[]", "append on root zero"),
            ("neg[-3]", "negative"),
            ("neg[]", "successor"),
            ("mixed[[b][c]]", "retained interior bracket"),
            ("x[__Host-a]", "exact prefix"),
        ] {
            bag.insert_unmangled(key, value.into())?;
        }
        assert_eq!(
            bag.into_value(),
            json!({
                " dot.key": "literal",
                "\0key": "nul",
                "x[broken": "literal bracket",
                "a": {"b": "tail discarded", "0": {"c": "last"}},
                "0": {"a": "empty root", "0": "append on root zero"},
                "neg": {"-3": "negative", "-2": "successor"},
                "mixed": [{"b[c": "retained interior bracket"}],
                "x": {"__Host-a": "exact prefix"}
            })
        );
        let mut input = VariableBag::default();
        input.insert(" dot.key", "normalized".into(), Registration::Input)?;
        input.insert("x[__Host-a]", "filtered".into(), Registration::Cookie)?;
        assert_eq!(
            input.into_value(),
            json!({"dot_key": "normalized", "x": []})
        );
        Ok(())
    }

    #[test]
    fn unmangled_limits_reject_oversized_paths_and_exhausted_appends() -> crate::Result<()> {
        let mut bag = VariableBag::default();
        let accepted = format!("deep{}", "[a]".repeat(MAX_DEPTH));
        bag.insert_unmangled(&accepted, "accepted".into())?;
        let rejected = format!("{accepted}[a]");
        assert!(bag.insert_unmangled(&rejected, "rejected".into()).is_err());

        let mut bag = VariableBag::default();
        bag.insert_unmangled("list[9223372036854775807]", "last".into())?;
        assert!(bag.insert_unmangled("list[]", "rejected".into()).is_err());
        assert_eq!(
            bag.into_value(),
            json!({"list": {"9223372036854775807": "last"}})
        );
        Ok(())
    }
}

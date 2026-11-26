use std::borrow::Cow;

/// the framework protects verbatim blocks before removing comments from the full source.
pub(super) struct PreparedSource<'a> {
    pub text: Cow<'a, str>,
    pub raw: Vec<String>,
}

impl<'a> PreparedSource<'a> {
    pub fn new(source: &'a str) -> Self {
        if !source.contains("@verbatim") && !source.contains("{{--") {
            return Self {
                text: Cow::Borrowed(source),
                raw: Vec::new(),
            };
        }
        let mut raw = Vec::new();
        let mut protected = String::new();
        let mut rest = source;
        let mut offset = 0;
        while let Some(index) = rest.find("@verbatim") {
            let start = offset + index;
            if start > 0 && source.as_bytes()[start - 1] == b'@' {
                let end = index + 9;
                protected.push_str(&rest[..end]);
                rest = &rest[end..];
                offset += end;
                continue;
            }
            let body = &rest[index + 9..];
            let Some(end) = body.find("@endverbatim") else {
                break;
            };
            let content = &body[..end];
            let whitespace = content
                .bytes()
                .take_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
                .count();
            protected.push_str(&rest[..index]);
            protected.push_str(&content[..whitespace]);
            protected.push_str(&format!("@__raw_block_{}__@", raw.len()));
            raw.push(content[whitespace..].to_owned());
            let consumed = index + 9 + end + 12;
            rest = &rest[consumed..];
            offset += consumed;
        }
        protected.push_str(rest);
        let mut uncommented = String::new();
        let mut rest = protected.as_str();
        while let Some(start) = rest.find("{{--") {
            let Some(end) = rest[start + 4..].find("--}}") else {
                break;
            };
            uncommented.push_str(&rest[..start]);
            rest = &rest[start + 4 + end + 4..];
        }
        uncommented.push_str(rest);
        Self {
            text: if uncommented == source {
                Cow::Borrowed(source)
            } else {
                Cow::Owned(uncommented)
            },
            raw,
        }
    }
}

// Raw content is restored after lexical boundaries have been identified. This also
// handles verbatim blocks inside quoted echo/directive/component expressions.
pub(super) fn restore_raw<'a>(source: &'a str, raw: &[String]) -> Cow<'a, str> {
    if raw.is_empty() || !source.contains("@__raw_block_") {
        return Cow::Borrowed(source);
    }
    let mut result = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("@__raw_block_") {
        result.push_str(&rest[..start]);
        let marker = &rest[start + 13..];
        if let Some((index, tail)) = marker.split_once("__@")
            && let Ok(index) = index.parse::<usize>()
            && let Some(value) = raw.get(index)
        {
            result.push_str(value);
            rest = tail;
        } else {
            result.push('@');
            rest = &rest[start + 1..];
        }
    }
    result.push_str(rest);
    Cow::Owned(result)
}

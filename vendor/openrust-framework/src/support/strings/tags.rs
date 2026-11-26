//! 按固定 PHP 8.5.0 php_strip_tags_ex 的状态机处理标签、引号和注释。
//! 来源 ext/standard/string.c，PHP License 3.01 见 THIRD_PARTY_NOTICES。

pub(super) fn strip(value: &str, allowed: &[&str]) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut tag = Vec::new();
    let (mut state, mut depth, mut quote, mut last, mut brackets, mut xml) =
        (0u8, 0usize, None, 0u8, 0i64, false);
    for (index, &byte) in bytes.iter().enumerate() {
        let previous = index
            .checked_sub(1)
            .and_then(|index| bytes.get(index))
            .copied();
        let whitespace_follows = bytes.get(index + 1).is_some_and(u8::is_ascii_whitespace);
        match state {
            0 => match byte {
                0 => {}
                b'<' if quote.is_some() => {}
                b'<' if whitespace_follows => output.push(byte),
                b'<' => {
                    last = byte;
                    state = 1;
                    if !allowed.is_empty() {
                        tag.push(byte);
                    }
                }
                b'>' if depth > 0 => depth -= 1,
                b'>' if quote.is_some() => {}
                _ => output.push(byte),
            },
            1 => match byte {
                0 => {}
                b'<' if quote.is_some() => {}
                b'<' if !whitespace_follows => depth += 1,
                b'>' if depth > 0 => depth -= 1,
                b'>' if quote.is_some() => {}
                b'>' => {
                    last = byte;
                    if xml && previous == Some(b'-') {
                        continue;
                    }
                    quote = None;
                    state = 0;
                    xml = false;
                    if !allowed.is_empty() {
                        tag.push(byte);
                        if permitted(&tag, allowed) {
                            output.extend_from_slice(&tag);
                        }
                        tag.clear();
                    }
                }
                b'\'' | b'"' => {
                    if quote.is_none() || quote == Some(byte) {
                        quote = if quote.is_some() { None } else { Some(byte) };
                    }
                    if !allowed.is_empty() {
                        tag.push(byte);
                    }
                }
                b'!' if previous == Some(b'<') => {
                    state = 3;
                    last = byte;
                }
                b'?' if previous == Some(b'<') => {
                    state = 2;
                    brackets = 0;
                }
                _ => {
                    if !allowed.is_empty() {
                        tag.push(byte);
                    }
                }
            },
            2 => match byte {
                b'(' if !matches!(last, b'\'' | b'"') => {
                    last = byte;
                    brackets += 1;
                }
                b')' if !matches!(last, b'\'' | b'"') => {
                    last = byte;
                    brackets -= 1;
                }
                b'>' if depth > 0 => depth -= 1,
                b'>' if quote.is_none()
                    && brackets == 0
                    && last != b'"'
                    && previous == Some(b'?') =>
                {
                    quote = None;
                    state = 0;
                    tag.clear();
                }
                b'\'' | b'"' if previous != Some(b'\\') => {
                    if last == byte {
                        last = 0;
                    } else if last != b'\\' {
                        last = byte;
                    }
                    if quote.is_none() || quote == Some(byte) {
                        quote = if quote.is_some() { None } else { Some(byte) };
                    }
                }
                b'l' | b'L'
                    if index > 4 && bytes[index - 4..=index].eq_ignore_ascii_case(b"<?xml") =>
                {
                    state = 1;
                    xml = true;
                }
                _ => {}
            },
            3 => match byte {
                b'>' if depth > 0 => depth -= 1,
                b'>' if quote.is_none() => {
                    quote = None;
                    state = 0;
                    tag.clear();
                }
                b'\'' | b'"' if previous != Some(b'\\') => {
                    if quote.is_none() || quote == Some(byte) {
                        quote = if quote.is_some() { None } else { Some(byte) };
                    }
                }
                b'-' if index >= 2 && &bytes[index - 2..index] == b"!-" => state = 4,
                b'e' | b'E'
                    if index > 6 && bytes[index - 6..=index].eq_ignore_ascii_case(b"doctype") =>
                {
                    state = 1
                }
                _ => {}
            },
            4 if byte == b'>'
                && quote.is_none()
                && index >= 2
                && &bytes[index - 2..index] == b"--" =>
            {
                quote = None;
                state = 0;
                tag.clear();
            }
            _ => {}
        }
    }
    String::from_utf8(output).expect("移除 ASCII 标签不会破坏 UTF-8 字符边界")
}

fn permitted(tag: &[u8], allowed: &[&str]) -> bool {
    let mut normalized = Vec::new();
    let mut started = false;
    for (index, &byte) in tag.iter().enumerate() {
        if byte == b'>' {
            break;
        }
        if byte == b'<' {
            continue;
        }
        if byte.is_ascii_whitespace() {
            if started {
                break;
            }
        } else {
            started = true;
            if byte != b'/'
                || (index > 0 && tag[index - 1] != b'<' && tag.get(index + 1) != Some(&b'>'))
            {
                normalized.push(byte);
            }
        }
    }
    allowed
        .iter()
        .any(|allowed| normalized.eq_ignore_ascii_case(allowed.trim_matches(['<', '>']).as_bytes()))
}

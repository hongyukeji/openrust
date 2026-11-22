//! Blade 原生块的共同词法边界；构建与运行使用同一规则，不解释 Rust。

use std::{io, ops::Range};

#[cfg(feature = "build")]
pub mod build;

/// 保留原始字节及模板行号，供代码生成与运行时一致性检查。
#[derive(Debug)]
pub struct Block<'a> {
    pub range: Range<usize>,
    pub body: &'a str,
    pub line: usize,
}

/// 提取可执行区域，先保护 Rust，避免 Blade 注释处理破坏 Rust 字符串。
pub fn blocks(source: &str) -> io::Result<Vec<Block<'_>>> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < source.len() {
        let rest = &source[i..];
        if rest.starts_with("{{--") {
            i += rest.find("--}}").map_or(rest.len(), |n| n + 4);
        } else if rest.starts_with("@@") {
            i += 2;
            while source.as_bytes().get(i).is_some_and(|b| word(*b)) {
                i += 1;
            }
        } else if directive(rest, "verbatim") {
            i += rest.find("@endverbatim").map_or(rest.len(), |n| n + 12);
        } else if rest.starts_with("{{") || rest.starts_with("{!!") {
            let (prefix, end) = if rest.starts_with("{{") {
                (2, "}}")
            } else {
                (3, "!!}")
            };
            i += prefix;
            while i < source.len() && !source[i..].starts_with(end) {
                i = quoted(source, i).unwrap_or(i + source[i..].chars().next().unwrap().len_utf8());
            }
            i = (i + end.len()).min(source.len());
        } else if directive(rest, "rust") {
            let start = i;
            i += 5;
            let body = i;
            let mut delimiters = Vec::new();
            while i < source.len() && !(delimiters.is_empty() && directive(&source[i..], "endrust"))
            {
                match source.as_bytes()[i] {
                    b'{' => delimiters.push(b'}'),
                    b'[' => delimiters.push(b']'),
                    b'(' => delimiters.push(b')'),
                    b'}' | b']' | b')' => {
                        delimiters.pop();
                    }
                    _ => {}
                }
                i = rust_token_end(source, i)?;
            }
            if i == source.len() {
                return Err(invalid(format!(
                    "第 {} 行的 @rust 缺少 @endrust",
                    line(source, start)
                )));
            }
            let end = i;
            i += 8;
            blocks.push(Block {
                range: start..i,
                body: &source[body..end],
                line: line(source, body),
            });
        } else if directive(rest, "endrust") {
            return Err(invalid(format!(
                "第 {} 行出现未配对的 @endrust",
                line(source, i)
            )));
        } else if rest.starts_with('@') {
            i += 1;
            while source.as_bytes().get(i).is_some_and(|b| word(*b)) {
                i += 1;
            }
            let spaces = source[i..].len() - source[i..].trim_start().len();
            if source.as_bytes().get(i + spaces) == Some(&b'(') {
                i += spaces + 1;
                let mut depth = 1;
                while i < source.len() && depth > 0 {
                    if let Some(end) = quoted(source, i) {
                        i = end;
                        continue;
                    }
                    match source.as_bytes()[i] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    i += source[i..].chars().next().unwrap().len_utf8();
                }
            }
        } else {
            i += rest.chars().next().unwrap().len_utf8();
        }
    }
    Ok(blocks)
}

/// 用节点占位符替代原生代码，后续 Blade 解析无需理解 Rust 词法。
pub fn mask(source: &str, blocks: &[Block<'_>]) -> String {
    let mut result = String::new();
    let mut start = 0;
    for (index, block) in blocks.iter().enumerate() {
        result.push_str(&source[start..block.range.start]);
        result.push_str(&format!("@__native_block_{index}__@"));
        start = block.range.end;
    }
    result.push_str(&source[start..]);
    result
}

fn word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
fn directive(source: &str, name: &str) -> bool {
    source
        .strip_prefix('@')
        .and_then(|s| s.strip_prefix(name))
        .is_some_and(|tail| !tail.as_bytes().first().is_some_and(|b| word(*b)))
}
fn line(source: &str, index: usize) -> usize {
    source[..index].bytes().filter(|b| *b == b'\n').count() + 1
}
fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

// 普通引号仅服务于 Blade 表达式；Rust 的字符／生命周期单独处理。
fn quoted(source: &str, i: usize) -> Option<usize> {
    let quote = *source.as_bytes().get(i)?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let mut j = i + 1;
    while j < source.len() {
        if source.as_bytes()[j] == quote {
            return Some(j + 1);
        }
        if source.as_bytes()[j] == b'\\' {
            j += 1;
        }
        j += 1;
    }
    Some(source.len())
}

fn rust_token_end(source: &str, i: usize) -> io::Result<usize> {
    let rest = &source[i..];
    if rest.starts_with("//") {
        return Ok(i + rest.find('\n').unwrap_or(rest.len()));
    }
    if rest.starts_with("/*") {
        let mut j = i + 2;
        let mut depth = 1;
        while j < source.len() && depth > 0 {
            if source[j..].starts_with("/*") {
                depth += 1;
                j += 2;
            } else if source[j..].starts_with("*/") {
                depth -= 1;
                j += 2;
            } else {
                j += source[j..].chars().next().unwrap().len_utf8();
            }
        }
        return Ok(j);
    }
    // 包括原始字节串／C 字符串。# 数量由结束引号严格匹配。
    for prefix in ["br", "cr", "r"] {
        if let Some(raw) = rest.strip_prefix(prefix) {
            let hashes = raw.bytes().take_while(|b| *b == b'#').count();
            if raw.as_bytes().get(hashes) == Some(&b'"') {
                let begin = i + prefix.len() + hashes + 1;
                let end = format!("\"{}", "#".repeat(hashes));
                return Ok(source[begin..]
                    .find(&end)
                    .map_or(source.len(), |n| begin + n + end.len()));
            }
        }
    }
    if rest.starts_with('"') {
        return Ok(quoted(source, i).unwrap());
    }
    if rest.starts_with('\'') {
        // 字符字面量在一个字符或一个转义之后闭合；其他形式交给 rustc 判定生命周期。
        let mut j = i + 1;
        if source.as_bytes().get(j) == Some(&b'\\') {
            j += 1;
            if source[j..].starts_with("u{") {
                j = source[j..].find('}').map_or(source.len(), |n| j + n + 1);
            } else if source.as_bytes().get(j) == Some(&b'x') {
                j = (j + 3).min(source.len());
            } else if j < source.len() {
                j += source[j..].chars().next().unwrap().len_utf8();
            }
        } else if j < source.len() {
            j += source[j..].chars().next().unwrap().len_utf8();
        }
        if source.as_bytes().get(j) == Some(&b'\'') {
            return Ok(j + 1);
        }
    }
    Ok(i + rest.chars().next().unwrap().len_utf8())
}

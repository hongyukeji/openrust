use std::sync::LazyLock;

use regex::Regex;

use super::{Str, display_width::DOUBLE_WIDTH};

pub(super) fn width(character: char) -> usize {
    let codepoint = u32::from(character);
    let index = DOUBLE_WIDTH.partition_point(|&(_, end)| end < codepoint);
    1 + usize::from(
        DOUBLE_WIDTH
            .get(index)
            .is_some_and(|&(start, _)| start <= codepoint),
    )
}

pub(super) fn limit(value: &str, columns: usize, end: &str, preserve_words: bool) -> String {
    if Str::width(value) <= columns {
        return value.to_owned();
    }
    let plain;
    let value = if preserve_words {
        static NEWLINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\n\r]+").unwrap());
        let stripped = super::editing::strip_tags(value, &[]);
        plain = NEWLINES.replace_all(&stripped, " ").into_owned();
        plain.trim_matches(php_space)
    } else {
        value
    };
    let mut remaining = columns;
    let stop = value
        .char_indices()
        .find_map(|(index, ch)| match remaining.checked_sub(width(ch)) {
            Some(next) => {
                remaining = next;
                None
            }
            None => Some(index),
        })
        .unwrap_or(value.len());
    let mut prefix = value[..stop].trim_end_matches(php_space);
    if preserve_words
        && value.chars().nth(columns) != Some(' ')
        && let Some((head, _)) = prefix.rsplit_once(|ch: char| ch.is_ascii_whitespace())
    {
        prefix = head;
    }
    format!("{prefix}{end}")
}

fn php_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\0' | '\u{b}')
}

/// Invalid line-breaking options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordWrapError {
    EmptyBreak,
    ZeroWidthCut,
}

impl std::fmt::Display for WordWrapError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyBreak => "line break must not be empty",
            Self::ZeroWidthCut => "forced wrapping requires a positive line width",
        })
    }
}

impl std::error::Error for WordWrapError {}

pub(super) fn wrap(
    value: &str,
    columns: usize,
    line_break: &str,
    cut: bool,
) -> Result<String, WordWrapError> {
    if value.is_empty() {
        return Ok(String::new());
    }
    if line_break.is_empty() {
        return Err(WordWrapError::EmptyBreak);
    }
    if columns == 0 && cut {
        return Err(WordWrapError::ZeroWidthCut);
    }

    let ascii = value.is_ascii();
    let indices: Vec<_> = value
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(value.len()))
        .collect();
    let mut output = String::with_capacity(value.len());
    let (mut start, mut last_space, mut current) = (0, None, 0);
    while current + 1 < indices.len() {
        let position = indices[current];
        if ascii && value[position..].starts_with(line_break) {
            current += line_break.len();
            output.push_str(&value[indices[start]..indices[current]]);
            start = current;
            last_space = None;
            continue;
        }

        if value.as_bytes()[position] == b' ' {
            if current - start >= columns {
                output.push_str(&value[indices[start]..position]);
                output.push_str(line_break);
                start = current + 1;
                last_space = None;
            } else {
                last_space = Some(current);
            }
        } else if current - start >= columns {
            if let Some(space) = last_space.filter(|&space| space > start) {
                output.push_str(&value[indices[start]..indices[space]]);
                output.push_str(line_break);
                start = space + 1;
                last_space = None;
            } else if cut {
                output.push_str(&value[indices[start]..position]);
                output.push_str(line_break);
                start = current;
                last_space = None;
            }
        }
        current += 1;
    }
    output.push_str(&value[indices[start]..]);
    Ok(output)
}

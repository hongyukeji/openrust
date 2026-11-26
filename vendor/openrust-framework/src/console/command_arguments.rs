// 第三方许可见 LICENSE.symfony。
use std::{collections::BTreeMap, ffi::OsString, sync::LazyLock};

use clap::{Arg, ArgAction, Command};
use regex::bytes::Regex;
use serde_json::Value;

use super::{
    ConsoleError,
    input::mutation::{emit_option, option, option_groups, scalar},
};

pub(crate) enum CommandArguments<'a> {
    Tokens(&'a [OsString]),
    Named(&'a [(&'a str, Value)]),
}

pub(super) fn named_arguments(
    parser: &Command,
    values: &[(&str, Value)],
) -> Result<Vec<OsString>, ConsoleError> {
    let mut provided = BTreeMap::new();
    for (name, value) in values {
        let matching = parser
            .get_arguments()
            .find(|argument| matches_name(argument, name))
            .ok_or_else(|| ConsoleError::new(format!("未定义命令参数：{name}")))?;
        if provided.insert(matching.get_id().as_str(), value).is_some() {
            return Err(ConsoleError::new(format!("命令参数重复：{name}")));
        }
    }

    let mut output = Vec::new();
    for arg in parser.get_arguments().filter(|arg| option(arg)) {
        let Some(value) = provided.get(arg.get_id().as_str()) else {
            continue;
        };
        match arg.get_action() {
            ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong | ArgAction::Version => {
                if value
                    .as_bool()
                    .ok_or_else(|| ConsoleError::new("帮助和版本选项需要布尔值"))?
                {
                    if let Some(name) = arg.get_long() {
                        output.push(format!("--{name}").into());
                    } else if let Some(name) = arg.get_short() {
                        output.push(format!("-{name}").into());
                    }
                }
            }
            ArgAction::SetTrue | ArgAction::SetFalse => {
                let groups = option_groups(arg, value)?;
                let enabled = value.as_bool().expect("已确认开关为布尔值");
                let flag_value = matches!(arg.get_action(), ArgAction::SetTrue);
                let default = arg
                    .get_default_values()
                    .first()
                    .and_then(|value| value.to_str())
                    .and_then(|value| value.parse::<bool>().ok())
                    .unwrap_or(!flag_value);
                if enabled != flag_value && enabled != default {
                    return Err(ConsoleError::new(format!(
                        "该值无法由原CLI定义表达：{}",
                        arg.get_id()
                    )));
                }
                emit_option(&mut output, arg, &groups)?;
            }
            _ => emit_option(&mut output, arg, &option_groups(arg, value)?)?,
        }
    }
    let mut positionals = parser.get_positionals().collect::<Vec<_>>();
    positionals.sort_by_key(|arg| arg.get_index());
    if let Some(last) = positionals
        .iter()
        .rposition(|arg| provided.contains_key(arg.get_id().as_str()))
    {
        output.push("--".into());
        for arg in &positionals[..=last] {
            match provided.get(arg.get_id().as_str()).copied() {
                Some(Value::Array(values))
                    if arg
                        .get_num_args()
                        .is_some_and(|range| range.max_values() > 1) =>
                {
                    output.extend(values.iter().map(scalar).collect::<Result<Vec<_>, _>>()?);
                }
                Some(Value::Array(_)) => {
                    return Err(ConsoleError::new(format!(
                        "位置参数只接受单值：{}",
                        arg.get_id()
                    )));
                }
                Some(value) => output.push(scalar(value)?),
                None if !arg.get_default_values().is_empty() => {
                    output.extend(arg.get_default_values().iter().map(OsString::from));
                }
                None => return Err(ConsoleError::new(format!("缺少位置参数：{}", arg.get_id()))),
            }
        }
    }
    Ok(output)
}

fn matches_name(arg: &Arg, name: &str) -> bool {
    if !option(arg) {
        return arg.get_id().as_str() == name;
    }
    if let Some(long) = name.strip_prefix("--") {
        return arg.get_long() == Some(long)
            || arg
                .get_all_aliases()
                .is_some_and(|aliases| aliases.contains(&long));
    }
    let Some(short) = name.strip_prefix('-') else {
        return false;
    };
    let mut characters = short.chars();
    characters.next().is_some_and(|short| {
        characters.next().is_none()
            && (arg.get_short() == Some(short)
                || arg
                    .get_all_short_aliases()
                    .is_some_and(|aliases| aliases.contains(&short)))
    })
}

/// 解析固定控制台字符串参数，不通过进程 Shell 执行。
fn tokenize(line: &str) -> Vec<Vec<u8>> {
    static QUOTED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?-u)^(?:"[^"\\]*(?:\\.[^"\\]*)*"|'[^'\\]*(?:\\.[^'\\]*)*')"#)
            .expect("合法命令引号表达式")
    });
    static COMPOUND: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r#"(?-u)^([^="'\s]+?)(=?)(?:"[^"\\]*(?:\\.[^"\\]*)*"|'[^'\\]*(?:\\.[^'\\]*)*')+"#,
        )
        .expect("合法具名命令值表达式")
    });
    let mut remaining = line.as_bytes();
    let mut tokens = Vec::new();
    let mut current = None;
    while let Some(&first) = remaining.first() {
        if first == b'\\' {
            let count = remaining.len().min(2);
            current
                .get_or_insert_with(Vec::new)
                .extend_from_slice(&remaining[1..count]);
            remaining = &remaining[count..];
        } else if matches!(first, b' ' | b'\t' | b'\n' | b'\r' | 11 | 12) {
            if let Some(token) = current.take() {
                tokens.push(token);
            }
            remaining = &remaining[1..];
        } else if let Some(matched) = COMPOUND.find(remaining) {
            let consumed = matched.end();
            let quotes = matched
                .as_bytes()
                .iter()
                .position(|byte| matches!(byte, b'"' | b'\''))
                .expect("组合值带有引号");
            let token = current.get_or_insert_with(Vec::new);
            token.extend_from_slice(&remaining[..quotes]);
            let quoted = &remaining[quotes + 1..consumed - 1];
            let mut joined = Vec::new();
            let mut bytes = quoted;
            while let Some((&byte, rest)) = bytes.split_first() {
                if matches!(byte, b'"' | b'\'')
                    && rest
                        .first()
                        .is_some_and(|next| matches!(next, b'"' | b'\''))
                {
                    bytes = &rest[1..];
                } else {
                    joined.push(byte);
                    bytes = rest;
                }
            }
            append_unescaped(token, &joined);
            remaining = &remaining[consumed..];
        } else if let Some(matched) = QUOTED.find(remaining) {
            let consumed = matched.end();
            append_unescaped(
                current.get_or_insert_with(Vec::new),
                &remaining[1..consumed - 1],
            );
            remaining = &remaining[consumed..];
        } else {
            // 普通连续字节可以一起接受；下一个特殊字符留给其自己的解析分支。
            let length = remaining
                .iter()
                .skip(1)
                .take_while(|byte| {
                    !matches!(
                        byte,
                        b'\\' | b'"' | b'\'' | b'=' | b' ' | b'\t' | b'\n' | b'\r' | 11 | 12
                    )
                })
                .count()
                + 1;
            current
                .get_or_insert_with(Vec::new)
                .extend_from_slice(&remaining[..length]);
            remaining = &remaining[length..];
        }
    }
    if let Some(token) = current {
        tokens.push(token);
    }
    tokens
}

fn append_unescaped(token: &mut Vec<u8>, mut input: &[u8]) {
    while let Some((&byte, rest)) = input.split_first() {
        input = rest;
        if byte != b'\\' {
            token.push(byte);
            continue;
        }
        let Some((&escaped, rest)) = input.split_first() else {
            token.push(byte);
            break;
        };
        input = rest;
        let decoded = match escaped {
            b'n' => b'\n',
            b'r' => b'\r',
            b'a' => 7,
            b't' => b'\t',
            b'v' => 11,
            b'b' => 8,
            b'f' => 12,
            b'x' => {
                let (value, len) = digits(input, 16, 2);
                if len == 0 {
                    b'x'
                } else {
                    input = &input[len..];
                    value
                }
            }
            b'0'..=b'7' => {
                let (value, len) = digits(&input[..input.len().min(2)], 8, 2);
                input = &input[len..];
                (escaped - b'0')
                    .wrapping_mul(8_u8.pow(len as u32))
                    .wrapping_add(value)
            }
            value => value,
        };
        token.push(decoded);
    }
}

fn digits(input: &[u8], radix: u32, maximum: usize) -> (u8, usize) {
    input
        .iter()
        .take(maximum)
        .map_while(|&byte| char::from(byte).to_digit(radix))
        .fold((0_u8, 0), |(value, count), digit| {
            (
                value.wrapping_mul(radix as u8).wrapping_add(digit as u8),
                count + 1,
            )
        })
}

pub(crate) fn line_arguments(line: &str) -> Result<(String, Vec<OsString>), ConsoleError> {
    let mut tokens = tokenize(line).into_iter();
    let command = tokens
        .next()
        .ok_or_else(|| ConsoleError::new("命令字符串为空"))?;
    let command = String::from_utf8(command).map_err(ConsoleError::from_source)?;
    #[cfg(unix)]
    let arguments = {
        use std::os::unix::ffi::OsStringExt as _;
        tokens.map(OsString::from_vec).collect()
    };
    #[cfg(not(unix))]
    let arguments = tokens
        .map(|bytes| {
            String::from_utf8(bytes)
                .map(OsString::from)
                .map_err(ConsoleError::from_source)
        })
        .collect::<Result<_, _>>()?;
    Ok((command, arguments))
}

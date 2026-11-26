use clap::{Arg, ArgAction, Command as Syntax};

use super::{CommandDefinition, ConsoleError};

impl CommandDefinition {
    /// 解析命令签名，参数仍交由同一Clap定义校验和派发。
    pub fn from_signature(signature: &'static str) -> Result<Self, ConsoleError> {
        let signature = signature.trim();
        let end = signature
            .find(char::is_whitespace)
            .unwrap_or(signature.len());
        let name = &signature[..end];
        if name.is_empty() || name.contains(['{', '}']) {
            return Err(ConsoleError::new("无法从签名确定命令名称"));
        }
        let mut syntax = Syntax::new(name).args_override_self(true);
        let mut rest = signature[end..].trim();
        let mut ids = std::collections::HashSet::new();
        let mut shortcuts = std::collections::HashSet::new();
        let mut optional = false;
        let mut variadic = false;
        while !rest.is_empty() {
            let Some(body) = rest.strip_prefix('{') else {
                return Err(ConsoleError::new("命令参数须置于花括号内"));
            };
            let Some(end) = body.find('}') else {
                return Err(ConsoleError::new("命令参数缺少结束花括号"));
            };
            let token = body[..end].trim();
            rest = body[end + 1..].trim();
            let description_at = token.char_indices().find_map(|(i, c)| {
                (c == ':'
                    && i > 0
                    && token[..i].ends_with(char::is_whitespace)
                    && token[i + 1..].starts_with(char::is_whitespace))
                .then_some(i)
            });
            let (token, description) =
                description_at.map_or((token, ""), |i| (&token[..i], &token[i + 1..]));
            let token = token.trim();
            let option = token.starts_with("--");
            let token = if option {
                token.trim_start_matches('-')
            } else {
                token
            };
            let (short, token) = if option {
                token
                    .split_once('|')
                    .map_or((None, token), |(short, token)| {
                        (Some(short.trim()), token.trim())
                    })
            } else {
                (None, token)
            };
            let (head, default) = token
                .split_once('=')
                .map_or((token, None), |(head, value)| (head, Some(value)));
            let array = head.ends_with('*') || default.is_some_and(|v| v.starts_with('*'));
            let id = head.trim_end_matches('*').trim_end_matches('?');
            if id.is_empty()
                || id.contains(|c: char| c.is_whitespace() || "{}|?*=".contains(c))
                || !ids.insert(id)
            {
                return Err(ConsoleError::new("命令参数名称无效或重复"));
            }
            let mut arg = Arg::new(id).help(description.trim());
            if option {
                arg = arg.long(id);
                if let Some(short) = short {
                    let mut chars = short.chars();
                    let c = chars
                        .next()
                        .ok_or_else(|| ConsoleError::new("选项缩写不能为空"))?;
                    if chars.next().is_some() || !c.is_ascii_alphanumeric() || !shortcuts.insert(c)
                    {
                        return Err(ConsoleError::new("选项缩写须为单个字母或数字"));
                    }
                    arg = arg.short(c);
                }
                if default.is_some() {
                    arg = arg.num_args(0..=1).require_equals(false);
                    if array {
                        arg = arg.action(ArgAction::Append);
                    }
                } else {
                    arg = arg.action(ArgAction::SetTrue);
                }
            } else {
                let required = !head.contains('?') && default.is_none();
                if variadic || (optional && required) {
                    return Err(ConsoleError::new(
                        "必填参数不能位于可选参数之后，数组参数须为最后一个位置参数",
                    ));
                }
                optional |= !required;
                variadic = array;
                arg = arg.required(required);
                if array {
                    arg = arg
                        .num_args(if required { 1.. } else { 0.. })
                        .action(ArgAction::Append);
                }
            }
            if let Some(default) = default {
                let default = default.strip_prefix('*').unwrap_or(default);
                if !default.is_empty() {
                    if array {
                        arg = arg.default_values(default.split(',').map(str::trim));
                    } else {
                        arg = arg.default_value(default);
                    }
                }
            }
            syntax = syntax.arg(arg);
        }
        Ok(Self::new(syntax))
    }
}

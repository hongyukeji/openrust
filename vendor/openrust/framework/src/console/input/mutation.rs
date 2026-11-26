use crate::console::{ConsoleError, Input};
use clap::{Arg, ArgAction, ArgMatches, parser::ValueSource};
use serde_json::Value;
use std::ffi::OsString;
type Groups = Vec<Vec<OsString>>;
enum Edit {
    Argument(Vec<OsString>),
    Option(Value),
}
impl Input {
    /// 按CLI参数重新校验位置值；失败时保持原输入，支持保留非UTF-8值。
    pub fn set_argument(
        &mut self,
        key: &str,
        values: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<(), ConsoleError> {
        self.edit(
            key,
            Edit::Argument(values.into_iter().map(Into::into).collect()),
        )
    }
    /// 更新可由CLI表达的选项值，仍由原Clap定义负责类型、约束及默认值。
    pub fn set_option(&mut self, key: &str, value: Value) -> Result<(), ConsoleError> {
        if key == "verbose" && self.inherited(key) && value.is_boolean() {
            return self.edit(
                key,
                Edit::Option(Value::from(u8::from(value.as_bool().unwrap()))),
            );
        }
        if key == "ansi" && self.inherited(key) {
            if !(value.is_boolean() || value.is_null()) {
                return Err(ConsoleError::new("ANSI选项需要布尔值或null"));
            }
            let mut candidate = self.clone();
            candidate.edit("ansi", Edit::Option(Value::Bool(false)))?;
            candidate.edit("no-ansi", Edit::Option(Value::Bool(false)))?;
            if let Some(enabled) = value.as_bool() {
                candidate.edit(
                    if enabled { "ansi" } else { "no-ansi" },
                    Edit::Option(Value::Bool(true)),
                )?;
            }
            *self = candidate;
            return Ok(());
        }
        self.edit(key, Edit::Option(value))
    }
    pub(crate) fn validate_prompted(&mut self, version: &'static str) -> Result<(), ConsoleError> {
        let mut parser = self
            .syntax
            .as_ref()
            .clone()
            .disable_help_flag(true)
            .disable_version_flag(true);
        if parser.get_version().is_none() {
            parser = parser.version(version);
        }
        for arg in self.parsing_syntax.get_arguments() {
            if !self
                .syntax
                .get_arguments()
                .any(|own| own.get_id() == arg.get_id())
            {
                parser = parser.arg(arg.clone());
            }
        }
        parser.build();
        let matches = parser
            .clone()
            .try_get_matches_from(tokens(self, &parser, None)?)
            .map_err(ConsoleError::from_source)?;
        self.matches = matches;
        self.parsing_syntax = std::sync::Arc::new(parser);
        Ok(())
    }
    fn edit(&mut self, key: &str, edit: Edit) -> Result<(), ConsoleError> {
        let mut parser = self.parsing_syntax.as_ref().clone();
        parser.build();
        let target = parser
            .get_arguments()
            .find(|arg| arg.get_id().as_str() == key)
            .ok_or_else(|| ConsoleError::new(format!("未定义输入：{key}")))?;
        if option(target) != matches!(edit, Edit::Option(_)) {
            return Err(ConsoleError::new(format!("输入分组不符：{key}")));
        }
        let expected = match &edit {
            Edit::Argument(values) => vec![values.clone()],
            Edit::Option(value) => option_groups(target, value)?,
        };
        let action = target.get_action().clone();
        let tokens = tokens(self, &parser, Some((key, &expected)))?;
        let matches = parser
            .try_get_matches_from(tokens)
            .map_err(ConsoleError::from_source)?;
        let candidate = Self {
            syntax: self.syntax.clone(),
            parsing_syntax: self.parsing_syntax.clone(),
            invoked_name: self.invoked_name.clone(),
            matches,
            shutdown: self.shutdown.clone(),
            helper_set: self.helper_set.clone(),
            component_dispatch: self.component_dispatch.clone(),
            interaction: self.interaction.clone(),
            stream: self.stream.clone(),
        };
        let equivalent = match &edit {
            Edit::Argument(values) => {
                groups(&candidate.matches, key)
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    == *values
            }
            Edit::Option(value)
                if matches!(
                    action,
                    ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count
                ) =>
            {
                candidate.raw_value(key).as_ref() == Some(value)
            }
            Edit::Option(_) => groups(&candidate.matches, key) == expected,
        };
        if !equivalent {
            return Err(ConsoleError::new(format!("该值无法由原CLI定义表达：{key}")));
        }
        for arg in self
            .parsing_syntax
            .get_arguments()
            .filter(|arg| arg.get_id().as_str() != key)
        {
            let id = arg.get_id().as_str();
            let value = self.raw_value(id);
            if value != candidate.raw_value(id)
                || (value.is_none() && groups(&self.matches, id) != groups(&candidate.matches, id))
            {
                return Err(ConsoleError::new(format!("修改会改变其他输入：{id}")));
            }
        }
        self.matches = candidate.matches;
        Ok(())
    }
}
fn option(arg: &Arg) -> bool {
    arg.get_long().is_some() || arg.get_short().is_some()
}
fn groups(matches: &ArgMatches, key: &str) -> Groups {
    matches
        .try_get_raw_occurrences(key)
        .ok()
        .flatten()
        .map(|groups| {
            groups
                .map(|group| group.map(OsString::from).collect())
                .collect()
        })
        .unwrap_or_default()
}
fn scalar(value: &Value) -> Result<OsString, ConsoleError> {
    match value {
        Value::String(value) => Ok(value.into()),
        Value::Number(_) | Value::Bool(_) => Ok(value.to_string().into()),
        _ => Err(ConsoleError::new("选项值必须是CLI标量")),
    }
}
fn option_groups(arg: &Arg, value: &Value) -> Result<Groups, ConsoleError> {
    match arg.get_action() {
        ArgAction::SetTrue | ArgAction::SetFalse => value
            .as_bool()
            .map(|value| vec![vec![value.to_string().into()]])
            .ok_or_else(|| ConsoleError::new("开关选项需要布尔值")),
        ArgAction::Count => value
            .as_u64()
            .filter(|value| *value <= u64::from(u8::MAX))
            .map(|value| vec![vec![value.to_string().into()]])
            .ok_or_else(|| ConsoleError::new("计数选项需要0至255的整数")),
        _ => match value {
            Value::Null => Ok(vec![Vec::new()]),
            Value::Array(values) if matches!(arg.get_action(), ArgAction::Append) => values
                .iter()
                .map(|value| {
                    if value.is_null() {
                        Ok(Vec::new())
                    } else {
                        Ok(vec![scalar(value)?])
                    }
                })
                .collect(),
            Value::Array(values) => Ok(vec![values.iter().map(scalar).collect::<Result<_, _>>()?]),
            _ => Ok(vec![vec![scalar(value)?]]),
        },
    }
}
fn emit_option(tokens: &mut Vec<OsString>, arg: &Arg, groups: &Groups) -> Result<(), ConsoleError> {
    if groups.is_empty() {
        return Ok(());
    }
    let name = arg
        .get_long()
        .map(|name| format!("--{name}"))
        .or_else(|| arg.get_short().map(|name| format!("-{name}")))
        .expect("选项具有名称");
    match arg.get_action() {
        ArgAction::SetTrue | ArgAction::SetFalse => {
            let enabled = groups
                .last()
                .and_then(|group| group.first())
                .is_some_and(|value| value == "true");
            if enabled == matches!(arg.get_action(), ArgAction::SetTrue) {
                tokens.push(name.into());
            }
        }
        ArgAction::Count => {
            let count = groups
                .last()
                .and_then(|group| group.first())
                .and_then(|value| value.to_str())
                .and_then(|value| value.parse::<u8>().ok())
                .ok_or_else(|| ConsoleError::new("无效的详细程度计数"))?;
            tokens.extend(std::iter::repeat_n(
                OsString::from(name),
                usize::from(count),
            ));
        }
        ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong | ArgAction::Version => {}
        _ => {
            for group in groups {
                if let Some((first, rest)) = group.split_first() {
                    let mut token = OsString::from(format!("{name}="));
                    token.push(first);
                    tokens.push(token);
                    tokens.extend(rest.iter().cloned());
                } else {
                    tokens.push(OsString::from(&name));
                }
            }
        }
    }
    Ok(())
}

fn tokens(
    input: &Input,
    parser: &clap::Command,
    edited: Option<(&str, &Groups)>,
) -> Result<Vec<OsString>, ConsoleError> {
    let mut tokens = vec![OsString::from(parser.get_name())];
    let mut positional = Vec::new();
    for (index, arg) in parser.get_arguments().enumerate() {
        let id = arg.get_id().as_str();
        let groups = if let Some((key, expected)) = edited
            && id == key
        {
            expected.clone()
        } else if input.matches.try_contains_id(id).is_ok()
            && input.matches.value_source(id) == Some(ValueSource::DefaultValue)
        {
            Vec::new()
        } else {
            groups(&input.matches, id)
        };
        if option(arg) {
            emit_option(&mut tokens, arg, &groups)?
        } else {
            positional.push((
                arg.get_index().unwrap_or(index + 1),
                groups.into_iter().flatten().collect::<Vec<_>>(),
            ));
        }
    }
    positional.sort_by_key(|(index, _)| *index);
    if positional.iter().any(|(_, values)| !values.is_empty()) {
        tokens.push("--".into());
        tokens.extend(positional.into_iter().flat_map(|(_, values)| values));
    }
    Ok(tokens)
}

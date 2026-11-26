use serde_json::{Map, Value};
use std::ffi::OsString;

use super::{CommandInput, ConsoleError, Input, Output, OutputStyle, OutputVerbosity};
use crate::support::Str;

/// 命令执行期间的输入／输出上下文，借用原输入及输出，不保存跨调用状态。
pub struct CommandIo<'a> {
    input: &'a Input,
    output: OutputStyle<'a>,
    components_output: Option<Box<OutputStyle<'a>>>,
    component_dispatch: Option<std::sync::Arc<dyn super::view::components::ComponentDispatch>>,
}
impl Input {
    pub fn io<'a>(&'a self, output: &'a mut Output) -> CommandIo<'a> {
        CommandIo::new(self, output)
    }
}
impl<'a> CommandIo<'a> {
    /// 在当前命令中收到指定信号时调用回调。
    pub fn trap(
        &self,
        signal: super::CommandSignal,
        callback: impl Fn(super::CommandSignal) + Send + Sync + 'static,
    ) -> Result<(), ConsoleError> {
        self.trap_many([signal], callback)
    }

    /// 用同一个回调处理多个信号。每种信号先执行最新注册的回调。
    pub fn trap_many(
        &self,
        signals: impl IntoIterator<Item = super::CommandSignal>,
        callback: impl Fn(super::CommandSignal) + Send + Sync + 'static,
    ) -> Result<(), ConsoleError> {
        super::command_signals::trap(signals, callback)
    }

    /// 清除本次命令注册的回调，保留外层命令的注册。
    pub fn untrap(&self) {
        super::command_signals::untrap();
    }

    /// 手动失败使用执行期间绑定的组件输出及 Artisan 退出码。
    pub fn fail<T>(&self, message: impl Into<String>) -> Result<T, ConsoleError> {
        self.input.fail(message)
    }

    pub fn fail_default<T>(&self) -> Result<T, ConsoleError> {
        self.input.fail_default()
    }

    pub fn get_helper_set(&self) -> Option<&super::HelperSet> {
        self.input.get_helper_set()
    }

    pub fn get_helper(
        &self,
        name: &str,
    ) -> Result<std::sync::Arc<dyn super::HelperInterface>, ConsoleError> {
        self.input.get_helper(name)
    }

    pub fn new(input: &'a Input, output: &'a mut Output) -> Self {
        Self {
            input,
            output: OutputStyle::new(input, output),
            components_output: None,
            component_dispatch: input.component_dispatch.clone(),
        }
    }
    pub fn has_argument(&self, name: &str) -> bool {
        self.input.has_argument(name)
    }
    pub fn has_option(&self, name: &str) -> bool {
        self.input.has_option(name)
    }
    pub fn input_value(&self, key: &str, default: Value) -> Value {
        self.input.input_value(key, default)
    }
    pub fn argument(&self, name: &str) -> Option<Value> {
        self.input.argument(name)
    }
    pub fn option(&self, name: &str) -> Option<Value> {
        self.input.option(name)
    }
    pub fn arguments(&self) -> Map<String, Value> {
        self.input.arguments()
    }
    pub fn options(&self) -> Map<String, Value> {
        self.input.options()
    }
    pub fn input(&self) -> CommandInput {
        self.input.input()
    }

    /// 组件工厂绑定创建时的样式；后续set_output仅更换普通输出入口。
    pub fn components(&mut self) -> super::view::components::Factory<'_, OutputStyle<'a>> {
        let output = self
            .components_output
            .as_deref_mut()
            .unwrap_or(&mut self.output);
        super::view::components::Factory::with_dispatch(output, self.component_dispatch.clone())
    }
    pub fn output_components(&mut self) -> super::view::components::Factory<'_, OutputStyle<'a>> {
        self.components()
    }

    /// 调用子命令，沿用当前输出和父命令可继承的全局选项。
    pub async fn call(
        &mut self,
        command: &str,
        arguments: &[OsString],
    ) -> Result<i32, ConsoleError> {
        let arguments = self.child_arguments(arguments);
        crate::support::facades::Artisan::call(command, &arguments, &mut self.output).await
    }

    /// 按具名参数调用子命令，显式子参数优先于继承的同名选项。
    pub async fn call_named(
        &mut self,
        command: &str,
        arguments: &[(&str, Value)],
    ) -> Result<i32, ConsoleError> {
        let mut merged = self.inherited_options();
        merged.retain(|(name, _)| {
            !arguments
                .iter()
                .any(|(provided, _)| option_overrides(name, provided))
        });
        merged.extend_from_slice(arguments);
        crate::support::facades::Artisan::call_named(command, &merged, &mut self.output).await
    }

    /// 调用子命令并丢弃它的输出，保留退出值。
    pub async fn call_silently(
        &mut self,
        command: &str,
        arguments: &[OsString],
    ) -> Result<i32, ConsoleError> {
        let arguments = self.child_arguments(arguments);
        crate::support::facades::Artisan::call_silently(command, &arguments).await
    }

    fn inherited_options(&self) -> Vec<(&'static str, Value)> {
        [
            ("--ansi", "ansi"),
            ("--no-ansi", "no-ansi"),
            ("--no-interaction", "no-interaction"),
            ("--quiet", "quiet"),
        ]
        .into_iter()
        .filter_map(|(flag, key)| {
            self.input
                .matches()
                .try_get_one::<bool>(key)
                .ok()
                .flatten()
                .copied()
                .unwrap_or(false)
                .then_some((flag, Value::Bool(true)))
        })
        .chain(
            self.input
                .matches()
                .try_get_one::<u8>("verbose")
                .ok()
                .flatten()
                .copied()
                .filter(|value| *value > 0)
                .map(|value| ("--verbose", Value::from(value))),
        )
        .collect()
    }

    fn child_arguments(&self, arguments: &[OsString]) -> Vec<OsString> {
        let mut merged = Vec::new();
        for (flag, value) in self.inherited_options() {
            if arguments
                .iter()
                .take_while(|arg| arg.as_os_str() != "--")
                .filter_map(|arg| arg.to_str())
                .any(|provided| {
                    option_overrides(flag, provided.split('=').next().unwrap_or(provided))
                })
            {
                continue;
            }
            if flag == "--verbose" {
                merged
                    .push(format!("-{}", "v".repeat(value.as_u64().unwrap_or(0) as usize)).into());
            } else {
                merged.push(flag.into());
            }
        }
        merged.extend_from_slice(arguments);
        merged
    }

    pub fn get_output(&mut self) -> &mut OutputStyle<'a> {
        &mut self.output
    }
    pub fn set_output(&mut self, output: OutputStyle<'a>) {
        let previous = std::mem::replace(&mut self.output, output);
        if self.components_output.is_none() {
            self.components_output = Some(Box::new(previous));
        }
    }
    pub fn set_input(&mut self, input: &'a Input) {
        self.input = input;
    }

    pub fn line(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.line_with(message, None, None)
    }
    pub fn line_with(
        &mut self,
        message: &str,
        style: Option<&str>,
        verbosity: Option<OutputVerbosity>,
    ) -> Result<(), ConsoleError> {
        let wrapped = style.filter(|style| !style.is_empty()).map_or_else(
            || message.to_owned(),
            |style| format!("<{style}>{message}</{style}>"),
        );
        self.output
            .writeln(wrapped.as_str(), verbosity.unwrap_or_default())
    }
    pub fn info(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.info_with(message, OutputVerbosity::Normal)
    }
    pub fn info_with(
        &mut self,
        message: &str,
        verbosity: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        self.line_with(message, Some("info"), Some(verbosity))
    }
    pub fn comment(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.comment_with(message, OutputVerbosity::Normal)
    }
    pub fn comment_with(
        &mut self,
        message: &str,
        verbosity: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        self.line_with(message, Some("comment"), Some(verbosity))
    }
    pub fn question(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.question_with(message, OutputVerbosity::Normal)
    }
    pub fn question_with(
        &mut self,
        message: &str,
        verbosity: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        self.line_with(message, Some("question"), Some(verbosity))
    }
    pub fn warn(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.warn_with(message, OutputVerbosity::Normal)
    }
    pub fn warn_with(
        &mut self,
        message: &str,
        verbosity: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        self.output.get_output().formatter().register_warning();
        self.line_with(message, Some("warning"), Some(verbosity))
    }
    pub fn error(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.error_with(message, OutputVerbosity::Normal)
    }
    pub fn error_with(
        &mut self,
        message: &str,
        verbosity: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        self.line_with(message, Some("error"), Some(verbosity))
    }
    pub fn alert(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.alert_with(message, OutputVerbosity::Normal)
    }
    pub fn alert_with(
        &mut self,
        message: &str,
        verbosity: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        let border = "*".repeat(Str::of(message).strip_tags(&[]).length() + 12);
        self.comment_with(&border, verbosity)?;
        self.comment_with(&format!("*     {message}     *"), verbosity)?;
        self.comment_with(&border, verbosity)?;
        self.comment_with("", verbosity)
    }
    pub fn new_line(&mut self, count: usize) -> Result<&mut Self, ConsoleError> {
        self.output.new_line(count)?;
        Ok(self)
    }
}

fn option_overrides(name: &str, provided: &str) -> bool {
    if name == provided {
        return true;
    }
    let short = provided.strip_prefix('-').filter(|value| {
        !value.starts_with('-') && value.chars().all(|flag| matches!(flag, 'q' | 'n' | 'v'))
    });
    match name {
        "--ansi" | "--no-ansi" => matches!(provided, "--ansi" | "--no-ansi"),
        "--quiet" => short.is_some_and(|flags| flags.contains('q')),
        "--no-interaction" => short.is_some_and(|flags| flags.contains('n')),
        "--verbose" => short.is_some_and(|flags| flags.contains('v')),
        _ => false,
    }
}

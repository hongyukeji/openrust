use serde_json::{Map, Value};

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

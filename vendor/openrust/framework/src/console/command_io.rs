use serde_json::{Map, Value};

use super::{CommandInput, ConsoleError, Input, Output};
use crate::support::Str;

/// 命令执行期间的输入／输出上下文，借用原输入及输出，不保存跨调用状态。
pub struct CommandIo<'a> {
    input: &'a Input,
    output: &'a mut Output,
}
impl Input {
    pub fn io<'a>(&'a self, output: &'a mut Output) -> CommandIo<'a> {
        CommandIo::new(self, output)
    }
}
impl<'a> CommandIo<'a> {
    pub fn new(input: &'a Input, output: &'a mut Output) -> Self {
        Self { input, output }
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

    /// 沿用原组件入口；后续实时写入前会先发出待写组件，保持顺序。
    pub fn components(&mut self) -> &mut Output {
        self.output
    }
    pub fn output_components(&mut self) -> &mut Output {
        self.components()
    }

    pub fn line(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.styled(message, None)
    }
    pub fn info(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.styled(message, Some("info"))
    }
    pub fn question(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.styled(message, Some("question"))
    }
    pub fn warn(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.output.formatter.register_warning();
        self.styled(message, Some("warning"))
    }
    pub fn error(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.styled(message, Some("error"))
    }
    pub fn comment(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.styled(message, Some("comment"))
    }

    pub fn alert(&mut self, message: &str) -> Result<(), ConsoleError> {
        let border = "*".repeat(Str::of(message).strip_tags(&[]).length() + 12);
        self.comment(&border)?;
        self.comment(&format!("*     {message}     *"))?;
        self.comment(&border)?;
        self.comment("")
    }
    pub fn new_line(&mut self, count: usize) -> Result<&mut Self, ConsoleError> {
        self.emit(&"\n".repeat(count))?;
        Ok(self)
    }
    fn styled(&mut self, message: &str, style: Option<&str>) -> Result<(), ConsoleError> {
        if self.output.suppressed {
            return Ok(());
        }
        let wrapped = style.map_or_else(
            || message.to_owned(),
            |style| format!("<{style}>{message}</{style}>"),
        );
        let mut rendered = self
            .output
            .formatter
            .format(&wrapped, self.output.is_decorated())
            .map_err(ConsoleError::from_source)?;
        rendered.push('\n');
        self.emit(&rendered)
    }
    fn emit(&mut self, message: &str) -> Result<(), ConsoleError> {
        if self.output.suppressed {
            return Ok(());
        }
        if self.output.streams_to_terminal() && !self.output.stdout.is_empty() {
            // 实时模式只保留待发内容，避免长任务累积整段终端历史。
            self.output.capture_generation = self.output.capture_generation.wrapping_add(1);
            let pending = std::mem::take(&mut self.output.stdout);
            if let Err(source) = self.output.write_raw(&pending) {
                self.output.stdout = pending;
                return Err(ConsoleError::from_source(source));
            }
        }
        self.output
            .write_raw(message)
            .map_err(ConsoleError::from_source)
    }
}

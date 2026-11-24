use super::{ConsoleError, Input, Output};
use crate::support::Str;

/// 输出阈值；消息只有在当前输出的级别不低于它时才会写出。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u16)]
pub enum OutputVerbosity {
    Silent = 8,
    Quiet = 16,
    #[default]
    Normal = 32,
    Verbose = 64,
    VeryVerbose = 128,
    Debug = 256,
}
impl OutputVerbosity {
    /// 命令消息使用的名称映射；未知名称沿用 normal 默认值。
    pub fn from_name(name: &str) -> Self {
        match name {
            "quiet" => Self::Quiet,
            "v" => Self::Verbose,
            "vv" => Self::VeryVerbose,
            "vvv" => Self::Debug,
            _ => Self::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u16)]
pub enum OutputFormat {
    #[default]
    Normal = 0,
    Raw = 2,
    Plain = 4,
}

/// 用独立字段表达格式和详细程度，避免整数位掩码混入其他选项。
#[derive(Clone, Copy, Debug, Default)]
pub struct OutputOptions {
    pub format: OutputFormat,
    pub verbosity: Option<OutputVerbosity>,
}
impl From<OutputVerbosity> for OutputOptions {
    fn from(verbosity: OutputVerbosity) -> Self {
        Self {
            verbosity: Some(verbosity),
            ..Self::default()
        }
    }
}
impl From<OutputFormat> for OutputOptions {
    fn from(format: OutputFormat) -> Self {
        Self {
            format,
            ..Self::default()
        }
    }
}

/// 单条字符串与逐条消息保留各自的换行规则，不提前拼接为同一条消息。
#[derive(Clone, Copy)]
pub enum OutputMessages<'a> {
    Text(&'a str),
    Lines(&'a [&'a str]),
}
impl<'a> From<&'a str> for OutputMessages<'a> {
    fn from(message: &'a str) -> Self {
        Self::Text(message)
    }
}
impl<'a> From<&'a [&'a str]> for OutputMessages<'a> {
    fn from(messages: &'a [&'a str]) -> Self {
        Self::Lines(messages)
    }
}
impl<'a, const N: usize> From<&'a [&'a str; N]> for OutputMessages<'a> {
    fn from(messages: &'a [&'a str; N]) -> Self {
        Self::Lines(messages)
    }
}
impl OutputMessages<'_> {
    fn trailing_new_lines(self) -> usize {
        match self {
            Self::Text(message) => message
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'\n')
                .count(),
            Self::Lines(messages) => trailing_iter_lines(messages.iter().copied()),
        }
    }
}

/// 命令的样式输出对象；包装本次调用的原输出及输入，不复制输出或建立全局状态。
pub struct OutputStyle<'a> {
    // 问答也使用构造时的输入；与 CommandIo::set_input 的读取对象独立。
    _input: &'a Input,
    output: &'a mut Output,
    new_lines_written: usize,
    new_line_written: bool,
}
impl<'a> OutputStyle<'a> {
    pub fn new(input: &'a Input, output: &'a mut Output) -> Self {
        Self {
            _input: input,
            output,
            new_lines_written: 1,
            new_line_written: false,
        }
    }
    pub fn get_output(&mut self) -> &mut Output {
        self.output
    }
    pub fn new_lines_written(&self) -> usize {
        self.new_lines_written
    }
    pub fn new_line_written(&self) -> bool {
        self.new_line_written
    }
    pub fn is_quiet(&self) -> bool {
        self.output.is_quiet()
    }
    pub fn is_verbose(&self) -> bool {
        self.output.is_verbose()
    }
    pub fn is_very_verbose(&self) -> bool {
        self.output.is_very_verbose()
    }
    pub fn is_debug(&self) -> bool {
        self.output.is_debug()
    }

    pub fn write<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        newline: bool,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        let messages = messages.into();
        self.new_lines_written = messages.trailing_new_lines() + usize::from(newline);
        self.new_line_written = self.new_lines_written > 0;
        self.write_messages(messages, newline, options.into())
    }
    pub fn writeln<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        let messages = messages.into();
        let options = options.into();
        let numeric = options.verbosity.map_or(0, |level| level as u16) | options.format as u16;
        if self.output.verbosity() as u16 >= numeric {
            self.new_lines_written = messages.trailing_new_lines() + 1;
            self.new_line_written = true;
        }
        self.write_messages(messages, true, options)
    }
    /// 接收单次遍历的字符串迭代器；先保存条目供换行统计，再按原顺序写出。
    pub fn write_iter<I, S>(
        &mut self,
        messages: I,
        newline: bool,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let messages: Vec<S> = messages.into_iter().collect();
        self.new_lines_written =
            trailing_iter_lines(messages.iter().map(AsRef::as_ref)) + usize::from(newline);
        self.new_line_written = self.new_lines_written > 0;
        self.write_iter_messages(messages.iter().map(AsRef::as_ref), newline, options.into())
    }
    pub fn writeln_iter<I, S>(
        &mut self,
        messages: I,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let messages: Vec<S> = messages.into_iter().collect();
        let options = options.into();
        let numeric = options.verbosity.map_or(0, |level| level as u16) | options.format as u16;
        if self.output.verbosity() as u16 >= numeric {
            self.new_lines_written = trailing_iter_lines(messages.iter().map(AsRef::as_ref)) + 1;
            self.new_line_written = true;
        }
        self.write_iter_messages(messages.iter().map(AsRef::as_ref), true, options)
    }
    fn write_iter_messages<'m>(
        &mut self,
        messages: impl IntoIterator<Item = &'m str>,
        newline: bool,
        options: OutputOptions,
    ) -> Result<(), ConsoleError> {
        let level = match options.verbosity {
            Some(OutputVerbosity::Silent) | None => OutputVerbosity::Normal,
            Some(level) => level,
        };
        if self.output.is_silent() || self.output.verbosity() < level {
            return Ok(());
        }
        for message in messages {
            self.write_message(message, newline, options.format, level)?;
        }
        Ok(())
    }
    pub fn new_line(&mut self, count: usize) -> Result<(), ConsoleError> {
        self.new_lines_written += count;
        self.new_line_written = self.new_lines_written > 0;
        self.output
            .emit_rendered(&"\n".repeat(count), OutputVerbosity::Normal)
    }
    fn write_messages(
        &mut self,
        messages: OutputMessages<'_>,
        newline: bool,
        options: OutputOptions,
    ) -> Result<(), ConsoleError> {
        match messages {
            OutputMessages::Text(message) => self.write_iter_messages([message], newline, options),
            OutputMessages::Lines(messages) => {
                self.write_iter_messages(messages.iter().copied(), newline, options)
            }
        }
    }
    fn write_message(
        &mut self,
        message: &str,
        newline: bool,
        format: OutputFormat,
        level: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        let mut rendered = match format {
            OutputFormat::Raw => message.to_owned(),
            OutputFormat::Normal | OutputFormat::Plain => self
                .output
                .formatter
                .format(message, self.output.is_decorated())
                .map_err(ConsoleError::from_source)?,
        };
        if format == OutputFormat::Plain {
            rendered = Str::of(rendered).strip_tags(&[]).to_string().to_owned();
        }
        if newline {
            rendered.push('\n');
        }
        self.output.emit_rendered(&rendered, level)
    }
}

fn trailing_iter_lines<'m>(messages: impl DoubleEndedIterator<Item = &'m str>) -> usize {
    let mut count = 0;
    for message in messages.rev() {
        count += 1;
        let trailing = message
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'\n')
            .count();
        count += trailing;
        if trailing < message.len() {
            break;
        }
    }
    count
}

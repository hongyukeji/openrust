use super::{ConsoleError, Input, Output};
use crate::support::Str;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
mod block;
mod table;
mod text;
pub use block::BlockOptions;
mod wrapping;

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
/// 子级借用父级期间，父级不能同时写入。
/// ```compile_fail
/// use openrust::console::{CommandDefinition, Output, OutputOptions, OutputStyle};
/// let input = CommandDefinition::from_signature("nested").unwrap().parse(&[]).unwrap();
/// let mut output = Output::default();
/// let mut parent = OutputStyle::new(&input, &mut output);
/// let mut child = OutputStyle::new(&input, &mut parent);
/// parent.writeln("competing", OutputOptions::default()).unwrap();
/// child.writeln("child", OutputOptions::default()).unwrap();
/// ```
pub struct OutputStyle<'a> {
    pub(super) section: Option<usize>,
    // 问答也使用构造时的输入；与 CommandIo::set_input 的读取对象独立。
    input: &'a Input,
    progress: Option<super::progress_bar::ProgressState>,
    line_length: i64,
    pub(super) state: StyleState,
    output: super::OutputReference<'a>,
}
pub(super) struct StyleState {
    history: Vec<u8>,
    history_formatter: super::formatting::Formatter,
    history_verbosity: OutputVerbosity,
    question_finally: Option<Arc<AtomicBool>>,
    new_lines_written: usize,
    new_line_written: bool,
}
impl StyleState {
    pub(super) fn count(&self) -> usize {
        self.new_lines_written
            + usize::from(
                self.question_finally
                    .as_ref()
                    .is_some_and(|flag| flag.load(Ordering::Relaxed)),
            )
    }
    pub(super) fn line_written(&self) -> bool {
        self.new_line_written
    }
    pub(super) fn update(
        &mut self,
        messages: OutputMessages<'_>,
        newline: bool,
        options: OutputOptions,
        writeln: bool,
        verbosity: OutputVerbosity,
    ) {
        self.settle_question();
        let numeric = options.verbosity.map_or(0, |level| level as u16) | options.format as u16;
        if !writeln || verbosity as u16 >= numeric {
            self.new_lines_written = messages.trailing_new_lines() + usize::from(newline);
            self.new_line_written = self.new_lines_written > 0;
        }
    }
    pub(super) fn buffer(
        &mut self,
        message: &str,
        newline: bool,
        options: OutputOptions,
    ) -> Result<(), ConsoleError> {
        let level = options
            .verbosity
            .filter(|v| *v != OutputVerbosity::Silent)
            .unwrap_or(OutputVerbosity::Normal);
        if self.history_verbosity == OutputVerbosity::Silent || self.history_verbosity < level {
            return Ok(());
        }
        let mut history = match options.format {
            OutputFormat::Raw => message.to_owned(),
            _ => self
                .history_formatter
                .format(message, false)
                .map_err(ConsoleError::from_source)?,
        };
        if options.format == OutputFormat::Plain {
            history = Str::of(history).strip_tags(&[]).to_string().to_owned();
        }
        if newline {
            history.push('\n');
        }
        self.record_history(&history);
        Ok(())
    }
}
impl StyleState {
    fn settle_question(&mut self) {
        if self
            .question_finally
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            self.new_lines_written += 1;
            self.question_finally = None;
        }
    }
    fn record_history(&mut self, text: &str) {
        if text.len() >= 2 {
            self.history.clear();
            self.history
                .extend_from_slice(&text.as_bytes()[text.len() - 2..]);
        } else if let Some(byte) = text.as_bytes().first() {
            if self.history.len() == 2 {
                self.history.remove(0);
            }
            self.history.push(*byte);
        }
    }
}
impl super::AsOutput for OutputStyle<'_> {
    fn as_output(&mut self) -> super::OutputReference<'_> {
        self.state.settle_question();
        let mut output = self.output.reborrow();
        output.parents.push(&mut self.state);
        output
    }
}
impl super::contracts::NewLineAware for OutputStyle<'_> {
    fn new_lines_written(&self) -> usize {
        self.new_lines_written()
    }
    fn new_line_written(&self) -> bool {
        self.new_line_written()
    }
}
impl<'a> OutputStyle<'a> {
    pub fn new(input: &'a Input, output: &'a mut (impl super::AsOutput + ?Sized)) -> Self {
        Self::from_reference(input, output.as_output())
    }
    fn from_reference(input: &'a Input, mut output: super::OutputReference<'a>) -> Self {
        let history_formatter = output.formatter().clone();
        let section = output.direct_section();
        let history_verbosity = output.get_verbosity();
        let width = super::Terminal::new().get_width();
        let line_length = (if width == 0 { 120 } else { width })
            .saturating_sub(i64::from(cfg!(windows)))
            .min(120);
        history_formatter.set_decorated(false);
        Self {
            input,
            section,
            progress: None,
            line_length,
            state: StyleState {
                history: Vec::new(),
                history_formatter,
                history_verbosity,
                question_finally: None,
                new_lines_written: 1,
                new_line_written: false,
            },
            output,
        }
    }
    pub fn set_verbosity(&mut self, level: OutputVerbosity) {
        self.output.set_verbosity(level);
    }
    pub fn get_verbosity(&self) -> OutputVerbosity {
        self.output.get_verbosity()
    }
    pub fn get_formatter(&self) -> super::FormatterHandle {
        self.output.get_formatter()
    }
    pub fn set_formatter(&mut self, formatter: super::FormatterHandle) {
        self.output.set_formatter(formatter);
    }
    pub fn set_decorated(&mut self, decorated: bool) {
        self.output.set_decorated(decorated);
    }
    pub fn is_decorated(&self) -> bool {
        self.output.is_decorated()
    }
    pub fn is_silent(&self) -> bool {
        self.output.is_silent()
    }
    pub fn get_output(&mut self) -> &mut super::OutputReference<'a> {
        &mut self.output
    }
    pub fn new_lines_written(&self) -> usize {
        self.output
            .parent_count()
            .unwrap_or_else(|| self.state.count())
    }
    pub fn new_line_written(&self) -> bool {
        self.state.line_written() || self.output.parent_line_written()
    }
    pub fn is_quiet(&self) -> bool {
        self.get_verbosity() == OutputVerbosity::Quiet
    }
    pub fn is_verbose(&self) -> bool {
        self.get_verbosity() >= OutputVerbosity::Verbose
    }
    pub fn is_very_verbose(&self) -> bool {
        self.get_verbosity() >= OutputVerbosity::VeryVerbose
    }
    pub fn is_debug(&self) -> bool {
        self.get_verbosity() == OutputVerbosity::Debug
    }

    pub fn write<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        newline: bool,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.settle_question();
        let messages = messages.into();
        self.state.new_lines_written = messages.trailing_new_lines() + usize::from(newline);
        self.state.new_line_written = self.state.new_lines_written > 0;
        self.write_messages(messages, newline, options.into(), false)
    }
    pub fn writeln<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.settle_question();
        let messages = messages.into();
        let options = options.into();
        let numeric = options.verbosity.map_or(0, |level| level as u16) | options.format as u16;
        if self.get_verbosity() as u16 >= numeric {
            self.state.new_lines_written = messages.trailing_new_lines() + 1;
            self.state.new_line_written = true;
        }
        self.write_messages(messages, true, options, true)
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
        self.settle_question();
        let messages: Vec<S> = messages.into_iter().collect();
        self.state.new_lines_written =
            trailing_iter_lines(messages.iter().map(AsRef::as_ref)) + usize::from(newline);
        self.state.new_line_written = self.state.new_lines_written > 0;
        self.write_iter_messages(
            messages.iter().map(AsRef::as_ref),
            newline,
            options.into(),
            false,
        )
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
        self.settle_question();
        let messages: Vec<S> = messages.into_iter().collect();
        let options = options.into();
        let numeric = options.verbosity.map_or(0, |level| level as u16) | options.format as u16;
        if self.get_verbosity() as u16 >= numeric {
            self.state.new_lines_written =
                trailing_iter_lines(messages.iter().map(AsRef::as_ref)) + 1;
            self.state.new_line_written = true;
        }
        self.write_iter_messages(messages.iter().map(AsRef::as_ref), true, options, true)
    }
    pub(super) fn question_input(&self) -> &Input {
        self.input
    }
    fn settle_question(&mut self) {
        self.state.settle_question();
    }
    pub(super) fn prepare_question(&mut self) -> Result<(), ConsoleError> {
        self.prepare_block()
    }
    pub(super) fn question_validation_error(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.new_line(1)?;
        self.error(message)
    }
    pub async fn ask_question(
        &mut self,
        question: &super::Question,
    ) -> Result<serde_json::Value, ConsoleError> {
        self.settle_question();
        let done = Arc::new(AtomicBool::new(false));
        self.state.question_finally = Some(done.clone());
        let finally = QuestionFinally(done);
        let shutdown = self.input.shutdown().cloned();
        let result = tokio::select! {
            biased;
            ()=async { match shutdown { Some(shutdown)=>shutdown.cancelled().await, None=>std::future::pending().await } }=>Err(ConsoleError::from_source(super::ConsoleQuestionError::Interrupted)),
            result=super::question::ask(self,question)=>result,
        };
        drop(finally);
        self.settle_question();
        result
    }
    fn write_iter_messages<'m>(
        &mut self,
        messages: impl IntoIterator<Item = &'m str>,
        newline: bool,
        options: OutputOptions,
        writeln: bool,
    ) -> Result<(), ConsoleError> {
        for message in messages {
            if writeln {
                self.output.writeln(message, options)?;
            } else {
                self.output.write(message, newline, options)?;
            }
            self.state.buffer(message, newline, options)?;
        }
        Ok(())
    }
    pub fn new_line(&mut self, count: usize) -> Result<(), ConsoleError> {
        self.settle_question();
        self.state.new_lines_written += count;
        self.state.new_line_written = self.state.new_lines_written > 0;
        let message = "\n".repeat(count);
        self.output
            .write(message.as_str(), false, OutputOptions::default())?;
        self.state.buffer(&message, false, OutputOptions::default())
    }
    fn write_messages(
        &mut self,
        messages: OutputMessages<'_>,
        newline: bool,
        options: OutputOptions,
        writeln: bool,
    ) -> Result<(), ConsoleError> {
        match messages {
            OutputMessages::Text(message) => {
                self.write_iter_messages([message], newline, options, writeln)
            }
            OutputMessages::Lines(messages) => {
                self.write_iter_messages(messages.iter().copied(), newline, options, writeln)
            }
        }
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

struct QuestionFinally(Arc<AtomicBool>);
impl Drop for QuestionFinally {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl Output {
    /// 底层格式化写入不更新外层样式历史，供直接输出的消费者复用。
    pub(super) fn emit_formatted(
        &mut self,
        message: &str,
        newline: bool,
        format: OutputFormat,
        level: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        if self.is_silent() || self.verbosity() < level {
            return Ok(());
        }
        let rendered = self.render_message(message, newline, format, false)?;
        self.emit_rendered(&rendered, level)?;
        Ok(())
    }
}

impl OutputStyle<'_> {
    pub fn progress_start(&mut self, max: i64) -> Result<(), ConsoleError> {
        let mut bar = self.create_progress_bar(max)?;
        let result = bar.start();
        let state = bar.into_state();
        self.progress = Some(state);
        result
    }
    pub fn progress_advance(&mut self) -> Result<(), ConsoleError> {
        self.progress_advance_by(1)
    }
    pub fn progress_advance_by(&mut self, steps: i64) -> Result<(), ConsoleError> {
        let state = self.progress.take().ok_or_else(|| {
            ConsoleError::from_source(super::ConsoleProgressError::MissingProgressBar)
        })?;
        let mut bar = super::ProgressBar::from_state(self.get_output(), state);
        let result = bar.advance_by(steps);
        let state = bar.into_state();
        self.progress = Some(state);
        result
    }
    pub fn progress_finish(&mut self) -> Result<(), ConsoleError> {
        let state = self.progress.take().ok_or_else(|| {
            ConsoleError::from_source(super::ConsoleProgressError::MissingProgressBar)
        })?;
        let mut bar = super::ProgressBar::from_state(self.get_output(), state);
        let result = bar.finish();
        let state = bar.into_state();
        if let Err(error) = result {
            self.progress = Some(state);
            return Err(error);
        }
        if let Err(error) = self.new_line(2) {
            self.progress = Some(state);
            return Err(error);
        }
        Ok(())
    }
}

impl Output {
    fn render_message(
        &mut self,
        message: &str,
        newline: bool,
        format: OutputFormat,
        error_stream: bool,
    ) -> Result<String, ConsoleError> {
        let mut rendered = if format == OutputFormat::Raw {
            message.to_owned()
        } else {
            let formatter = if error_stream {
                self.progress_formatter()
            } else {
                &mut self.formatter
            };
            formatter
                .format_current(message)
                .map_err(ConsoleError::from_source)?
        };
        if format == OutputFormat::Plain {
            rendered = Str::of(rendered).strip_tags(&[]).to_string().to_owned();
        }
        if newline {
            rendered.push('\n');
        }
        Ok(rendered)
    }
    pub(super) fn progress_verbosity(&self) -> OutputVerbosity {
        if self.terminal {
            self.error_verbosity.unwrap_or_else(|| self.verbosity())
        } else {
            self.verbosity()
        }
    }
    pub(super) fn progress_is_decorated(&self) -> bool {
        if self.terminal {
            self.error_formatter.as_ref().map_or(
                self.error_decorated,
                super::formatting::Formatter::is_decorated,
            )
        } else {
            self.is_decorated()
        }
    }
    pub(super) fn progress_formatter(&mut self) -> &mut super::formatting::Formatter {
        if self.terminal {
            self.error_formatter.get_or_insert_with(|| {
                let formatter = super::formatting::Formatter::default();
                formatter.set_decorated(self.error_decorated);
                formatter
            })
        } else {
            &mut self.formatter
        }
    }
    /// 实际控制台的进度写入stderr；缓冲调用仍沿用单一输出，不建立第二份捕获结果。
    pub(super) fn emit_progress(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.emit_progress_message(
            message,
            false,
            OutputFormat::Normal,
            OutputVerbosity::Normal,
        )
    }
    pub(super) fn emit_progress_message(
        &mut self,
        message: &str,
        newline: bool,
        format: OutputFormat,
        level: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        if self.progress_verbosity() == OutputVerbosity::Silent || self.progress_verbosity() < level
        {
            return Ok(());
        }
        if !self.terminal {
            return self.emit_formatted(message, newline, format, level);
        }
        self.emit_rendered("", level)?;
        let rendered = self.render_message(message, newline, format, true)?;
        use std::io::Write as _;
        let mut stderr = std::io::stderr().lock();
        if !self.stderr.is_empty() {
            stderr
                .write_all(self.stderr.as_bytes())
                .map_err(ConsoleError::from_source)?;
            self.stderr.clear();
        }
        stderr
            .write_all(rendered.as_bytes())
            .and_then(|()| stderr.flush())
            .map_err(ConsoleError::from_source)
    }
}

impl<'a> OutputStyle<'a> {
    pub(super) fn for_section(input: &'a Input, output: &'a mut Output, id: usize) -> Self {
        Self::from_reference(
            input,
            super::OutputReference {
                output,
                section: Some(id),
                parents: Vec::new(),
            },
        )
    }
}

impl OutputStyle<'_> {
    pub(super) fn root_output(&mut self) -> &mut Output {
        self.output.output
    }
    pub(super) fn section_input_submit(&mut self) -> Result<(), ConsoleError> {
        if let Some(id) = self.section {
            let section = &mut self.output.output.sections[id];
            section.input_submit()?;
        }
        Ok(())
    }
}

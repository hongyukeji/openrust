use super::{ConsoleError, Input, Output};
use crate::support::Str;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
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
pub struct OutputStyle<'a> {
    pub(super) section: Option<usize>,
    // 问答也使用构造时的输入；与 CommandIo::set_input 的读取对象独立。
    input: &'a Input,
    progress: Option<super::progress_bar::ProgressState>,
    history: Vec<u8>,
    history_formatter: super::formatting::Formatter,
    question_finally: Option<Arc<AtomicBool>>,
    output: &'a mut Output,
    new_lines_written: usize,
    new_line_written: bool,
}
impl<'a> OutputStyle<'a> {
    pub fn new(input: &'a Input, output: &'a mut Output) -> Self {
        let history_formatter = output.formatter.clone();
        history_formatter.set_decorated(false);
        Self {
            input,
            section: None,
            progress: None,
            history: Vec::new(),
            history_formatter,
            question_finally: None,
            output,
            new_lines_written: 1,
            new_line_written: false,
        }
    }
    pub fn set_verbosity(&mut self, level: OutputVerbosity) {
        if let Some(id) = self.section {
            self.output.sections[id].verbosity = level;
        } else {
            self.output.set_verbosity(level);
        }
    }
    pub fn get_verbosity(&self) -> OutputVerbosity {
        self.section.map_or_else(
            || self.output.verbosity(),
            |id| self.output.sections[id].verbosity,
        )
    }
    pub fn get_formatter(&self) -> super::FormatterHandle {
        self.section.map_or_else(
            || self.output.get_formatter(),
            |id| self.output.sections[id].formatter.handle(),
        )
    }
    pub fn set_formatter(&mut self, formatter: super::FormatterHandle) {
        if let Some(id) = self.section {
            self.output.sections[id].formatter =
                super::formatting::Formatter::from_handle(formatter);
        } else {
            self.output.set_formatter(formatter);
        }
    }
    pub fn set_decorated(&mut self, decorated: bool) {
        if let Some(id) = self.section {
            self.output.sections[id].formatter.set_decorated(decorated);
        } else {
            self.output.set_decorated(decorated);
        }
    }
    pub fn is_decorated(&self) -> bool {
        self.section.map_or_else(
            || self.output.is_decorated(),
            |id| self.output.sections[id].formatter.is_decorated(),
        )
    }
    pub fn is_silent(&self) -> bool {
        self.get_verbosity() == OutputVerbosity::Silent
    }

    pub fn get_output(&mut self) -> &mut Output {
        self.output
    }
    pub fn new_lines_written(&self) -> usize {
        self.new_lines_written
            + usize::from(
                self.question_finally
                    .as_ref()
                    .is_some_and(|flag| flag.load(Ordering::Relaxed)),
            )
    }
    pub fn new_line_written(&self) -> bool {
        self.new_line_written
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
        self.new_lines_written = messages.trailing_new_lines() + usize::from(newline);
        self.new_line_written = self.new_lines_written > 0;
        self.write_messages(messages, newline, options.into())
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
        self.settle_question();
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
        self.settle_question();
        let messages: Vec<S> = messages.into_iter().collect();
        let options = options.into();
        let numeric = options.verbosity.map_or(0, |level| level as u16) | options.format as u16;
        if self.get_verbosity() as u16 >= numeric {
            self.new_lines_written = trailing_iter_lines(messages.iter().map(AsRef::as_ref)) + 1;
            self.new_line_written = true;
        }
        self.write_iter_messages(messages.iter().map(AsRef::as_ref), true, options)
    }
    pub(super) fn question_input(&self) -> &Input {
        self.input
    }
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
    pub(super) fn prepare_question(&mut self) -> Result<(), ConsoleError> {
        let count = if self.history.is_empty() {
            1
        } else {
            2 - self.history.iter().filter(|byte| **byte == b'\n').count()
        };
        self.history.clear();
        self.new_line(count)
    }
    pub(super) fn question_validation_error(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.new_line(1)?;
        self.prepare_question()?;
        let width = std::env::var("COLUMNS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(|| usize::from(::console::Term::stdout().size().1))
            .min(120);
        let width = width.saturating_sub(usize::from(cfg!(windows))).max(9);
        let escaped = message.replace('<', "\\<");
        let mut lines = wrapping::wrap(&escaped, width - 9)?;
        let first = usize::from(self.is_decorated());
        if first == 1 {
            lines.insert(0, String::new());
            lines.push(String::new());
        }
        for (index, line) in lines.iter().enumerate() {
            let label = if index == first {
                "[ERROR] "
            } else {
                "        "
            };
            let body = format!(" {label}{line}");
            let visible = super::formatting::Formatter::default()
                .format(&body, false)
                .map_err(ConsoleError::from_source)?;
            let padding = " ".repeat(width.saturating_sub(::console::measure_text_width(&visible)));
            self.writeln(
                format!("<fg=white;bg=red>{body}{padding}</>").as_str(),
                OutputOptions::default(),
            )?;
        }
        self.new_line(1)
    }
    pub async fn ask_question(
        &mut self,
        question: &super::Question,
    ) -> Result<serde_json::Value, ConsoleError> {
        self.settle_question();
        let done = Arc::new(AtomicBool::new(false));
        self.question_finally = Some(done.clone());
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
    ) -> Result<(), ConsoleError> {
        let level = match options.verbosity {
            Some(OutputVerbosity::Silent) | None => OutputVerbosity::Normal,
            Some(level) => level,
        };
        if self.is_silent() || self.get_verbosity() < level {
            return Ok(());
        }
        for message in messages {
            self.write_message(message, newline, options.format, level)?;
        }
        Ok(())
    }
    pub fn new_line(&mut self, count: usize) -> Result<(), ConsoleError> {
        self.settle_question();
        self.new_lines_written += count;
        self.new_line_written = self.new_lines_written > 0;
        let message = "\n".repeat(count);
        if let Some(id) = self.section {
            self.output.emit_section(
                id,
                &message,
                false,
                OutputFormat::Raw,
                OutputVerbosity::Normal,
            )?;
        } else {
            self.output
                .emit_rendered(&message, OutputVerbosity::Normal)?;
        }
        if self.get_verbosity() >= OutputVerbosity::Normal {
            self.record_history(&message);
        }
        Ok(())
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
        if let Some(id) = self.section {
            self.output
                .emit_section(id, message, newline, format, level)?;
        } else {
            self.output
                .emit_formatted(message, newline, format, level)?;
        }
        let mut history = match format {
            OutputFormat::Raw => message.to_owned(),
            _ => self
                .history_formatter
                .format(message, false)
                .map_err(ConsoleError::from_source)?,
        };
        if format == OutputFormat::Plain {
            history = Str::of(history).strip_tags(&[]).to_string().to_owned();
        }
        if newline {
            history.push('\n');
        }
        self.record_history(&history);
        Ok(())
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

impl OutputStyle<'_> {
    pub(super) fn clear_table_section(&mut self, lines: i64) -> Result<(), ConsoleError> {
        let id = self
            .section
            .ok_or_else(|| ConsoleError::new("追加并重绘表格需要控制台分区输出"))?;
        self.output.clear_section(id, Some(lines))
    }
}

impl<'a> OutputStyle<'a> {
    pub(super) fn for_section(input: &'a Input, output: &'a mut Output, id: usize) -> Self {
        let mut style = Self::new(input, output);
        style.section = Some(id);
        style.history_formatter = style.output.sections[id].formatter.clone();
        style.history_formatter.set_decorated(false);
        style
    }
}

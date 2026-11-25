use super::{
    ConsoleError, FormatterHandle, Output, OutputFormat, OutputMessages, OutputOptions,
    OutputVerbosity,
};
/// 借用当前选中的输出；保留分区身份，避免消费者退回普通流。
///
/// 可读取原捕获缓冲，但不能取得可变原流以绕开所选分区。
/// ```compile_fail
/// use openrust::console::{Output, OutputReference};
/// let mut output = Output::default();
/// let mut selected = OutputReference::from(&mut output);
/// let raw: &mut Output = &mut *selected;
/// ```
pub struct OutputReference<'a> {
    pub(super) output: &'a mut Output,
    pub(super) section: Option<usize>,
    pub(super) parents: Vec<&'a mut super::output_style::StyleState>,
}
/// 将输出作为短期可变借用提供给现有消费者；不转移调用方的借用。
pub trait AsOutput {
    fn as_output(&mut self) -> OutputReference<'_>;
}
impl AsOutput for Output {
    fn as_output(&mut self) -> OutputReference<'_> {
        OutputReference {
            output: self,
            section: None,
            parents: Vec::new(),
        }
    }
}
impl AsOutput for OutputReference<'_> {
    fn as_output(&mut self) -> OutputReference<'_> {
        self.reborrow()
    }
}
impl<T: AsOutput + ?Sized> AsOutput for Box<T> {
    fn as_output(&mut self) -> OutputReference<'_> {
        (**self).as_output()
    }
}
impl AsOutput for super::ConsoleSectionOutput<'_> {
    fn as_output(&mut self) -> OutputReference<'_> {
        super::ConsoleSectionOutput::as_output(self)
    }
}
impl<'a> From<&'a mut Output> for OutputReference<'a> {
    fn from(output: &'a mut Output) -> Self {
        Self {
            output,
            section: None,
            parents: Vec::new(),
        }
    }
}
impl std::ops::Deref for OutputReference<'_> {
    type Target = Output;
    fn deref(&self) -> &Output {
        self.output
    }
}
impl OutputReference<'_> {
    pub(super) fn formatter(&mut self) -> &mut super::formatting::Formatter {
        if let Some(id) = self.section {
            &mut self.output.sections[id].formatter
        } else {
            &mut self.output.formatter
        }
    }
    pub(super) fn reborrow(&mut self) -> OutputReference<'_> {
        OutputReference {
            output: self.output,
            section: self.section,
            parents: self.parents.iter_mut().map(|state| &mut **state).collect(),
        }
    }
    pub(super) fn for_style_table(&mut self) -> OutputReference<'_> {
        if self.parents.is_empty() && self.section.is_none() && self.output.streams_to_terminal() {
            let id = self.output.new_section_id();
            OutputReference {
                output: self.output,
                section: Some(id),
                parents: Vec::new(),
            }
        } else {
            self.reborrow()
        }
    }
    pub(super) fn direct_section(&self) -> Option<usize> {
        self.parents.is_empty().then_some(self.section).flatten()
    }
    pub(super) fn parent_count(&self) -> Option<usize> {
        self.parents.first().map(|state| state.count())
    }
    pub(super) fn parent_line_written(&self) -> bool {
        self.parents.iter().any(|state| state.line_written())
    }
    /// 继续借用被包装样式的底层输出；普通流没有下一层样式。
    pub fn get_output(&mut self) -> Option<OutputReference<'_>> {
        if self.parents.is_empty() {
            return None;
        }
        let mut output = self.reborrow();
        output.parents.pop();
        Some(output)
    }
    pub fn as_new_line_aware(&self) -> Option<NewLineView<'_, '_>> {
        (!self.parents.is_empty()).then_some(NewLineView {
            states: &self.parents,
        })
    }
    pub fn as_section(&mut self) -> Result<super::ConsoleSectionOutput<'_>, ConsoleError> {
        let id = self
            .direct_section()
            .ok_or_else(|| ConsoleError::new("当前输出不是控制台分区"))?;
        Ok(super::ConsoleSectionOutput::from_reference(self.output, id))
    }
    pub fn get_formatter(&self) -> FormatterHandle {
        self.section.map_or_else(
            || self.output.get_formatter(),
            |id| self.output.sections[id].formatter.handle(),
        )
    }
    pub fn set_formatter(&mut self, formatter: FormatterHandle) {
        if let Some(id) = self.section {
            self.output.sections[id].formatter =
                super::formatting::Formatter::from_handle(formatter);
        } else {
            self.output.set_formatter(formatter);
        }
    }
    pub fn verbosity(&self) -> OutputVerbosity {
        self.get_verbosity()
    }
    pub fn get_verbosity(&self) -> OutputVerbosity {
        self.section.map_or_else(
            || self.output.verbosity(),
            |id| self.output.sections[id].verbosity,
        )
    }
    pub fn set_verbosity(&mut self, level: OutputVerbosity) {
        if let Some(id) = self.section {
            self.output.sections[id].verbosity = level;
        } else {
            self.output.set_verbosity(level);
        }
    }
    pub fn is_decorated(&self) -> bool {
        self.get_formatter().is_decorated()
    }
    pub fn set_decorated(&mut self, decorated: bool) {
        if self.section.is_some() {
            self.get_formatter().set_decorated(decorated);
        } else {
            self.output.set_decorated(decorated);
        }
    }
    pub fn is_silent(&self) -> bool {
        self.get_verbosity() == OutputVerbosity::Silent
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
        self.forward(messages.into(), newline, options.into(), false)
    }
    fn forward(
        &mut self,
        messages: OutputMessages<'_>,
        newline: bool,
        options: OutputOptions,
        writeln: bool,
    ) -> Result<(), ConsoleError> {
        forward(
            self.output,
            self.section,
            &mut self.parents,
            messages,
            newline,
            options,
            writeln,
        )
    }
    pub fn write_raw(&mut self, message: &str) -> std::io::Result<()> {
        if self.parents.is_empty() && self.section.is_none() {
            return self.output.write_raw(message);
        }
        self.write(message, false, OutputFormat::Raw)
            .map_err(std::io::Error::other)
    }
    pub fn writeln<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.forward(messages.into(), true, options.into(), true)
    }
    pub(super) fn emit_input_echo(&mut self, message: &str) -> Result<(), ConsoleError> {
        if self.section.is_some() {
            if self.get_verbosity() < OutputVerbosity::Normal || self.is_silent() {
                return Ok(());
            }
            self.output.emit_to_sink(message)
        } else {
            self.output.emit_rendered(message, OutputVerbosity::Normal)
        }
    }
    pub(super) fn progress_verbosity(&self) -> OutputVerbosity {
        if self.section.is_some() || !self.parents.is_empty() {
            self.get_verbosity()
        } else {
            self.output.progress_verbosity()
        }
    }
    pub(super) fn progress_is_decorated(&self) -> bool {
        if self.section.is_some() || !self.parents.is_empty() {
            self.is_decorated()
        } else {
            self.output.progress_is_decorated()
        }
    }
    pub(super) fn progress_formatter(&mut self) -> &mut super::formatting::Formatter {
        if !self.parents.is_empty() {
            return self.formatter();
        }
        if let Some(id) = self.section {
            &mut self.output.sections[id].formatter
        } else {
            self.output.progress_formatter()
        }
    }
    pub(super) fn set_progress_decorated(&mut self, value: bool) {
        if self.section.is_some() || !self.parents.is_empty() {
            self.set_decorated(value)
        } else {
            self.output.set_progress_decorated(value)
        }
    }
    pub(super) fn set_progress_verbosity(&mut self, level: OutputVerbosity) {
        if self.parents.is_empty() && self.section.is_none() && self.output.streams_to_terminal() {
            self.output.error_verbosity = Some(level);
        } else {
            self.set_verbosity(level)
        }
    }
    pub(super) fn set_progress_formatter(&mut self, formatter: FormatterHandle) {
        if self.parents.is_empty() && self.section.is_none() && self.output.streams_to_terminal() {
            self.output.error_formatter =
                Some(super::formatting::Formatter::from_handle(formatter));
            self.output.error_formatter_generation =
                self.output.error_formatter_generation.wrapping_add(1);
        } else {
            self.set_formatter(formatter)
        }
    }
    pub(super) fn emit_progress(&mut self, message: &str) -> Result<(), ConsoleError> {
        if !self.parents.is_empty() {
            return self.write(message, false, OutputOptions::default());
        }
        if self.section.is_none() {
            self.output.emit_progress(message)
        } else {
            self.emit_progress_message(
                message,
                false,
                OutputFormat::Normal,
                OutputVerbosity::Normal,
            )
        }
    }
    pub(super) fn emit_progress_message(
        &mut self,
        message: &str,
        newline: bool,
        format: OutputFormat,
        level: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        if !self.parents.is_empty() {
            return self.write(
                message,
                newline,
                OutputOptions {
                    format,
                    verbosity: Some(level),
                },
            );
        }
        if let Some(id) = self.section {
            self.output
                .emit_section(id, message, newline, format, level)
        } else {
            self.output
                .emit_progress_message(message, newline, format, level)
        }
    }
    pub(super) fn clear_section(&mut self, count: i64) -> Result<(), ConsoleError> {
        let id = self
            .direct_section()
            .ok_or_else(|| ConsoleError::new("当前输出不是控制台分区"))?;
        self.output.clear_section(id, Some(count))
    }
}

/// 底层样式的只读换行能力；普通输出流不提供此能力。
pub struct NewLineView<'view, 'state> {
    states: &'view [&'state mut super::output_style::StyleState],
}
impl super::contracts::NewLineAware for NewLineView<'_, '_> {
    fn new_lines_written(&self) -> usize {
        self.states.first().map_or(0, |state| state.count())
    }
    fn new_line_written(&self) -> bool {
        self.states.iter().any(|state| state.line_written())
    }
}
fn forward(
    output: &mut Output,
    section: Option<usize>,
    states: &mut [&mut super::output_style::StyleState],
    messages: OutputMessages<'_>,
    newline: bool,
    options: OutputOptions,
    writeln: bool,
) -> Result<(), ConsoleError> {
    if let Some((state, parents)) = states.split_last_mut() {
        let verbosity =
            section.map_or_else(|| output.verbosity(), |id| output.sections[id].verbosity);
        state.update(messages, newline, options, writeln, verbosity);
        let mut one = |message: &str| -> Result<(), ConsoleError> {
            forward(
                output,
                section,
                parents,
                OutputMessages::Text(message),
                newline,
                options,
                writeln,
            )?;
            state.buffer(message, newline, options)
        };
        match messages {
            OutputMessages::Text(text) => one(text),
            OutputMessages::Lines(lines) => {
                for text in lines {
                    one(text)?;
                }
                Ok(())
            }
        }
    } else {
        let level = options
            .verbosity
            .filter(|v| *v != OutputVerbosity::Silent)
            .unwrap_or(OutputVerbosity::Normal);
        let mut one = |message: &str| match section {
            Some(id) => output.emit_section(id, message, newline, options.format, level),
            None => output.emit_formatted(message, newline, options.format, level),
        };
        match messages {
            OutputMessages::Text(text) => one(text),
            OutputMessages::Lines(lines) => {
                for text in lines {
                    one(text)?;
                }
                Ok(())
            }
        }
    }
}

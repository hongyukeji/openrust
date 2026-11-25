use super::{
    ConsoleError, FormatterHandle, Input, Output, OutputFormat, OutputMessages, OutputOptions,
    OutputStyle, OutputVerbosity,
};

pub(super) struct SectionState {
    content: Vec<String>,
    lines: i64,
    max_height: i64,
    pub(super) verbosity: OutputVerbosity,
    pub(super) formatter: super::formatting::Formatter,
}
/// 借用原输出流的控制台分区；后建分区的内容随前区清除／重写而重排。
pub struct ConsoleSectionOutput<'a> {
    output: &'a mut Output,
    id: usize,
}
/// 可重新借用原输出分区的身份；不能用于其他输出实例。
#[derive(Clone)]
pub struct ConsoleSectionHandle {
    id: usize,
    owner: std::sync::Arc<()>,
}
impl Output {
    pub fn section_output(
        &mut self,
        handle: &ConsoleSectionHandle,
    ) -> Result<ConsoleSectionOutput<'_>, ConsoleError> {
        if !self
            .section_identity
            .as_ref()
            .is_some_and(|owner| std::sync::Arc::ptr_eq(owner, &handle.owner))
        {
            return Err(ConsoleError::new("分区句柄不属于当前输出"));
        }
        Ok(ConsoleSectionOutput {
            output: self,
            id: handle.id,
        })
    }

    pub fn section(&mut self) -> ConsoleSectionOutput<'_> {
        let id = self.new_section_id();
        ConsoleSectionOutput { output: self, id }
    }
    pub(super) fn new_section_id(&mut self) -> usize {
        self.section_identity
            .get_or_insert_with(|| std::sync::Arc::new(()));
        let id = self.sections.len();
        self.sections.push(SectionState {
            content: Vec::new(),
            lines: 0,
            max_height: 0,
            verbosity: self.verbosity(),
            formatter: super::formatting::Formatter::from_handle(self.get_formatter()),
        });
        id
    }
    fn section_pop(&mut self, id: usize, own_lines: i64) -> Result<String, ConsoleError> {
        let mut lines = own_lines;
        let mut contents = Vec::new();
        for section in self.sections[id + 1..].iter().rev() {
            lines = lines
                .checked_add(if section.max_height != 0 {
                    section.lines.min(section.max_height)
                } else {
                    section.lines
                })
                .ok_or_else(|| ConsoleError::new("分区行数超出可表示范围"))?;
            let mut content = section.visible();
            if !content.is_empty() {
                if !content.ends_with('\n') {
                    content.push('\n');
                }
                contents.push(content);
            }
        }
        if lines > 0 {
            self.emit_to_sink(&format!("\x1b[{lines}A\x1b[0J"))?;
        }
        contents.reverse();
        Ok(contents.concat())
    }
    pub(super) fn clear_section(
        &mut self,
        id: usize,
        lines: Option<i64>,
    ) -> Result<(), ConsoleError> {
        if self.sections[id].content.is_empty() || !self.sections[id].formatter.is_decorated() {
            return Ok(());
        }
        let section = &mut self.sections[id];
        let count = match lines.filter(|n| *n != 0) {
            Some(n) => {
                let start = offset(section.content.len(), n.saturating_neg());
                section.content.truncate(start);
                n
            }
            None => {
                let count = section.lines;
                section.content.clear();
                count
            }
        };
        section.lines = section
            .lines
            .checked_sub(count)
            .ok_or_else(|| ConsoleError::new("分区行数超出可表示范围"))?;
        let count = if section.max_height != 0 {
            count.min(section.max_height)
        } else {
            count
        };
        let after = self.section_pop(id, count)?;
        self.emit_to_sink(&after)
    }
    pub(super) fn emit_section(
        &mut self,
        id: usize,
        message: &str,
        newline: bool,
        format: OutputFormat,
        level: OutputVerbosity,
    ) -> Result<(), ConsoleError> {
        let section = &mut self.sections[id];
        if section.verbosity == OutputVerbosity::Silent || section.verbosity < level {
            return Ok(());
        }
        let mut message = match format {
            OutputFormat::Raw => message.to_owned(),
            _ => section
                .formatter
                .format_current(message)
                .map_err(ConsoleError::from_source)?,
        };
        if format == OutputFormat::Plain {
            message = crate::support::Str::of(message)
                .strip_tags(&[])
                .to_string()
                .to_owned();
        }
        let mut newline = newline;
        if !newline && message.ends_with('\n') {
            message.pop();
            newline = true;
        }
        if !self.sections[id].formatter.is_decorated() {
            if newline {
                message.push('\n');
            }
            return self.emit_to_sink(&message);
        }
        let last = self.sections[id]
            .content
            .last()
            .filter(|line| !line.ends_with('\n'))
            .cloned();
        let added = self.section_add_content(id, &message, newline)?;
        let section = &self.sections[id];
        let overflow = section.max_height > 0 && section.lines > section.max_height;
        let clear = if overflow {
            section.max_height
        } else {
            i64::from(last.is_some())
        };
        let after = self.section_pop(id, clear)?;
        if overflow {
            let section = &self.sections[id];
            let start = offset(section.content.len(), section.lines - section.max_height);
            let length = section.max_height - added;
            let end = if length >= 0 {
                start
                    .saturating_add(length as usize)
                    .min(section.content.len())
            } else {
                offset(section.content.len(), length).max(start)
            };
            self.emit_to_sink(&self.sections[id].content[start..end].concat())?;
        }
        let mut emitted = last.unwrap_or_default();
        emitted.push_str(&message);
        emitted.push('\n');
        self.emit_to_sink(&emitted)?;
        self.emit_to_sink(&after)
    }
    fn section_add_content(
        &mut self,
        id: usize,
        message: &str,
        newline: bool,
    ) -> Result<i64, ConsoleError> {
        let width = super::Terminal::new().get_width();
        let parts: Vec<_> = message.split('\n').collect();
        let mut added = 0;
        for (index, line) in parts.iter().enumerate() {
            let mut line = (*line).to_owned();
            if index + 1 < parts.len() || newline {
                line.push('\n');
            }
            if line.is_empty() {
                continue;
            }
            let old = (index == 0)
                .then(|| self.sections[id].content.last())
                .flatten()
                .filter(|line| !line.ends_with('\n'))
                .cloned();
            if let Some(old) = old {
                self.sections[id].lines -= self.section_line_count(id, &old, width)?;
                line = old + &line;
                *self.sections[id].content.last_mut().expect("前一行存在") = line.clone();
            } else {
                self.sections[id].content.push(line.clone());
            }
            added += self.section_line_count(id, &line, width)?;
        }
        self.sections[id].lines = self.sections[id]
            .lines
            .checked_add(added)
            .ok_or_else(|| ConsoleError::new("分区行数超出可表示范围"))?;
        Ok(added)
    }
    fn section_line_count(&self, id: usize, text: &str, width: i64) -> Result<i64, ConsoleError> {
        if width == 0 {
            return Err(ConsoleError::new("终端宽度为零，无法计算分区行数"));
        }
        let text = text.replace('\t', "        ");
        let mut formatter =
            super::formatting::Formatter::from_handle(self.sections[id].formatter.handle());
        let text = super::formatting::unformatted_text(&mut formatter, &text)
            .map_err(ConsoleError::from_source)?;
        let count = (super::formatting::display_width(&text) as f64 / width as f64).ceil() as i64;
        Ok(if count == 0 { 1 } else { count })
    }
}
fn offset(len: usize, value: i64) -> usize {
    if value >= 0 {
        (value as usize).min(len)
    } else {
        len.saturating_sub(value.unsigned_abs() as usize)
    }
}
impl SectionState {
    pub(super) fn input_submit(&mut self) -> Result<(), ConsoleError> {
        self.content.push("\n".into());
        self.lines = self
            .lines
            .checked_add(1)
            .ok_or_else(|| ConsoleError::new("分区行数超出可表示范围"))?;
        Ok(())
    }
    fn visible(&self) -> String {
        let start = if self.max_height == 0 {
            0
        } else {
            offset(self.content.len(), self.max_height.saturating_neg())
        };
        self.content[start..].concat()
    }
}
impl ConsoleSectionOutput<'_> {
    pub(super) fn from_reference(output: &mut Output, id: usize) -> ConsoleSectionOutput<'_> {
        ConsoleSectionOutput { output, id }
    }

    pub fn as_output(&mut self) -> super::OutputReference<'_> {
        super::OutputReference {
            output: self.output,
            section: Some(self.id),
            parents: Vec::new(),
        }
    }

    pub fn handle(&self) -> ConsoleSectionHandle {
        ConsoleSectionHandle {
            id: self.id,
            owner: self
                .output
                .section_identity
                .as_ref()
                .expect("分区身份已初始化")
                .clone(),
        }
    }

    /// 以原生借用绑定既有样式／表格消费者，不复制输出流。
    pub fn style<'a>(&'a mut self, input: &'a Input) -> OutputStyle<'a> {
        OutputStyle::for_section(input, self.output, self.id)
    }
    pub fn get_content(&self) -> String {
        self.output.sections[self.id].content.concat()
    }
    pub fn get_visible_content(&self) -> String {
        self.output.sections[self.id].visible()
    }
    pub fn clear(&mut self, lines: Option<i64>) -> Result<(), ConsoleError> {
        self.output.clear_section(self.id, lines)
    }
    pub fn set_max_height(&mut self, height: i64) -> Result<(), ConsoleError> {
        let section = &mut self.output.sections[self.id];
        let previous = section.max_height;
        section.max_height = height;
        let count = if previous != 0 {
            previous.min(section.lines)
        } else {
            section.lines
        };
        let after = self.output.section_pop(self.id, count)?;
        self.output.emit_to_sink(&self.get_visible_content())?;
        self.output.emit_to_sink(&after)
    }
    pub fn write<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.write_messages(messages.into(), false, options.into())
    }
    pub fn writeln<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.write_messages(messages.into(), true, options.into())
    }
    pub fn overwrite<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.clear(None)?;
        self.writeln(messages, OutputOptions::default())
    }
    fn write_messages(
        &mut self,
        messages: OutputMessages<'_>,
        newline: bool,
        options: OutputOptions,
    ) -> Result<(), ConsoleError> {
        let level = options
            .verbosity
            .filter(|v| *v != OutputVerbosity::Silent)
            .unwrap_or(OutputVerbosity::Normal);
        match messages {
            OutputMessages::Text(text) => {
                self.output
                    .emit_section(self.id, text, newline, options.format, level)
            }
            OutputMessages::Lines(lines) => {
                for text in lines {
                    self.output
                        .emit_section(self.id, text, newline, options.format, level)?;
                }
                Ok(())
            }
        }
    }
    pub fn get_formatter(&self) -> FormatterHandle {
        self.output.sections[self.id].formatter.handle()
    }
    pub fn set_formatter(&mut self, formatter: FormatterHandle) {
        self.output.sections[self.id].formatter =
            super::formatting::Formatter::from_handle(formatter);
    }
    pub fn is_decorated(&self) -> bool {
        self.output.sections[self.id].formatter.is_decorated()
    }
    pub fn set_decorated(&mut self, decorated: bool) {
        self.output.sections[self.id]
            .formatter
            .set_decorated(decorated);
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
    pub fn write_iter<I, S>(
        &mut self,
        messages: I,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let options = options.into();
        for message in messages {
            self.write(message.as_ref(), options)?;
        }
        Ok(())
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
        let options = options.into();
        for message in messages {
            self.writeln(message.as_ref(), options)?;
        }
        Ok(())
    }
    pub fn get_verbosity(&self) -> OutputVerbosity {
        self.output.sections[self.id].verbosity
    }
    pub fn set_verbosity(&mut self, verbosity: OutputVerbosity) {
        self.output.sections[self.id].verbosity = verbosity;
    }
}

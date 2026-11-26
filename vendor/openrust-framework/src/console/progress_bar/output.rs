use super::{ConsoleError, OutputReference, OutputVerbosity, ProgressBar};
use crate::console::{OutputMessages, OutputOptions};

/// 当前进度的实际输出流借用；真实控制台为stderr，捕获调用为原缓冲。
pub struct ProgressOutput<'a> {
    output: OutputReference<'a>,
}
impl ProgressBar<'_> {
    /// 在同一可变借用中访问输出，避免将进度条与其输出重复借出。
    pub fn get_output(&mut self) -> ProgressOutput<'_> {
        ProgressOutput {
            output: self.output.reborrow(),
        }
    }
}
impl ProgressOutput<'_> {
    pub fn as_section(&mut self) -> Result<super::super::ConsoleSectionOutput<'_>, ConsoleError> {
        self.output.as_section()
    }

    pub fn write<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        newline: bool,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        let options = options.into();
        match messages.into() {
            OutputMessages::Text(message) => self.write_one(message, newline, options),
            OutputMessages::Lines(messages) => self.write_iter(messages, newline, options),
        }
    }
    pub fn writeln<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.write(messages, true, options)
    }
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
        let options = options.into();
        for message in messages {
            self.write_one(message.as_ref(), newline, options)?;
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
        self.write_iter(messages, true, options)
    }
    fn write_one(
        &mut self,
        message: &str,
        newline: bool,
        options: OutputOptions,
    ) -> Result<(), ConsoleError> {
        self.output.emit_progress_message(
            message,
            newline,
            options.format,
            options.verbosity.unwrap_or_default(),
        )
    }
    pub fn set_verbosity(&mut self, level: OutputVerbosity) {
        self.output.set_progress_verbosity(level);
    }
    pub fn set_decorated(&mut self, decorated: bool) {
        self.output.set_progress_decorated(decorated);
    }
    pub fn get_formatter(&mut self) -> super::super::FormatterHandle {
        self.output.progress_formatter().handle()
    }
    pub fn set_formatter(&mut self, formatter: super::super::FormatterHandle) {
        self.output.set_progress_formatter(formatter);
    }

    pub fn get_verbosity(&self) -> OutputVerbosity {
        self.output.progress_verbosity()
    }
    pub fn is_decorated(&self) -> bool {
        self.output.progress_is_decorated()
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
}

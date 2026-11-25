use super::{ConsoleError, OutputMessages, OutputOptions, OutputStyle};

impl OutputStyle<'_> {
    /// 输出主标题与等号下划线。
    pub fn title(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.heading(message, "=")
    }
    /// 输出分节标题；与 Output::section 创建可重绘分区的职责不同。
    pub fn section(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.heading(message, "-")
    }
    fn heading(&mut self, message: &str, marker: &str) -> Result<(), ConsoleError> {
        self.prepare_block()?;
        let plain = super::super::formatting::unformatted_text(self.output.formatter(), message)
            .map_err(ConsoleError::from_source)?;
        let width = super::super::formatting::display_width(&plain);
        let message = super::super::OutputFormatter::escape_trailing_backslash(message);
        self.writeln_iter(
            [
                format!("<comment>{message}</>"),
                format!("<comment>{}</>", marker.repeat(width)),
            ],
            OutputOptions::default(),
        )?;
        self.new_line(1)
    }
    /// 逐条输出带星号的列表；空列表仍保留末尾空行。
    pub fn listing<I, S>(&mut self, elements: I) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.prepare_text()?;
        self.writeln_iter(
            elements
                .into_iter()
                .map(|element| format!(" * {}", element.as_ref())),
            OutputOptions::default(),
        )?;
        self.new_line(1)
    }
    /// 输出单条或多条带缩进的文本，不添加块级末尾空行。
    pub fn text<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.prepare_text()?;
        let mut line =
            |message: &str| self.writeln(format!(" {message}").as_str(), OutputOptions::default());
        match messages.into() {
            OutputMessages::Text(message) => line(message),
            OutputMessages::Lines(messages) => {
                for message in messages {
                    line(message)?;
                }
                Ok(())
            }
        }
    }
    pub(super) fn prepare_block(&mut self) -> Result<(), ConsoleError> {
        let count = if self.state.history.is_empty() {
            1
        } else {
            2 - self
                .state
                .history
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
        };
        self.state.history.clear();
        self.new_line(count)
    }
    fn prepare_text(&mut self) -> Result<(), ConsoleError> {
        let newline = self.state.history.last().is_some_and(|byte| *byte != b'\n');
        self.state.history.clear();
        if newline {
            self.new_line(1)?;
        }
        Ok(())
    }
}

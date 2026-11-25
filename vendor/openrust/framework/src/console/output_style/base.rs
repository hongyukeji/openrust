use crate::console::{
    AsOutput, BlockOptions, ChoiceKey, CompletionCallback, ConsoleError, ConsoleFuture, Input,
    OutputMessages, OutputOptions, OutputStyle, OutputVerbosity, ProgressBar,
    ProgressStyleIterator, QuestionCallback, StyleInterface, TableHeaders, TableRow,
};
use serde_json::Value;

/// 基础命令样式；共享正式渲染机制，不提供派生样式的换行计数接口。
/// ```compile_fail,E0277
/// use openrust::console::{CommandDefinition, ConsoleStyle, Output, contracts::NewLineAware};
/// fn needs_count<T: NewLineAware>(_: &T) {}
/// let input = CommandDefinition::from_signature("base").unwrap().parse(&[]).unwrap();
/// let mut output = Output::default();
/// let style = ConsoleStyle::new(&input, &mut output);
/// needs_count(&style);
/// ```
/// ```compile_fail,E0499
/// use openrust::console::{CommandDefinition, Output, OutputStyle};
/// let input = CommandDefinition::from_signature("errors").unwrap().parse(&[]).unwrap();
/// let mut output = Output::default();
/// let mut original = OutputStyle::new(&input, &mut output);
/// let mut errors = original.get_error_style();
/// original.text("competing").unwrap();
/// errors.text("selected").unwrap();
/// ```
/// ```compile_fail,E0599
/// use openrust::console::{CommandDefinition, ConsoleStyle, Output};
/// let input = CommandDefinition::from_signature("base").unwrap().parse(&[]).unwrap();
/// let mut output = Output::default();
/// let style = ConsoleStyle::new(&input, &mut output);
/// let count = style.new_lines_written();
/// ```
pub struct ConsoleStyle<'output> {
    inner: OutputStyle<'output>,
}
impl<'output> ConsoleStyle<'output> {
    pub fn new(input: &'output Input, output: &'output mut (impl AsOutput + ?Sized)) -> Self {
        Self::from_reference(input, output.as_output())
    }
    fn from_reference(
        input: &'output Input,
        output: crate::console::OutputReference<'output>,
    ) -> Self {
        let mut inner = OutputStyle::from_reference(input, output);
        inner.state.new_line_aware = false;
        Self { inner }
    }
    pub fn get_error_style(&mut self) -> ConsoleStyle<'_> {
        self.inner.get_error_style()
    }
    pub fn set_verbosity(&mut self, level: OutputVerbosity) {
        self.inner.set_verbosity(level)
    }
    pub fn get_verbosity(&self) -> OutputVerbosity {
        self.inner.get_verbosity()
    }
    pub fn get_formatter(&self) -> crate::console::FormatterHandle {
        self.inner.get_formatter()
    }
    pub fn set_formatter(&mut self, formatter: crate::console::FormatterHandle) {
        self.inner.set_formatter(formatter)
    }
    pub fn set_decorated(&mut self, decorated: bool) {
        self.inner.set_decorated(decorated)
    }
    pub fn is_decorated(&self) -> bool {
        self.inner.is_decorated()
    }
    pub fn is_silent(&self) -> bool {
        self.inner.is_silent()
    }
    pub fn is_quiet(&self) -> bool {
        self.inner.is_quiet()
    }
    pub fn is_verbose(&self) -> bool {
        self.inner.is_verbose()
    }
    pub fn is_very_verbose(&self) -> bool {
        self.inner.is_very_verbose()
    }
    pub fn is_debug(&self) -> bool {
        self.inner.is_debug()
    }
    pub fn write<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        newline: bool,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.inner.write(messages, newline, options)
    }
    pub fn writeln<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.inner.writeln(messages, options)
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
            self.inner.write(message.as_ref(), newline, options)?;
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
            self.inner.writeln(message.as_ref(), options)?;
        }
        Ok(())
    }
    pub async fn ask_question(
        &mut self,
        question: &crate::console::Question,
    ) -> Result<serde_json::Value, ConsoleError> {
        self.inner.ask_question(question).await
    }
    pub fn new_line(&mut self, count: usize) -> Result<(), ConsoleError> {
        self.inner.new_line(count)
    }
    pub fn progress_start(&mut self, max: i64) -> Result<(), ConsoleError> {
        self.inner.progress_start(max)
    }
    pub fn progress_advance(&mut self) -> Result<(), ConsoleError> {
        self.inner.progress_advance()
    }
    pub fn progress_advance_by(&mut self, steps: i64) -> Result<(), ConsoleError> {
        self.inner.progress_advance_by(steps)
    }
    pub fn progress_finish(&mut self) -> Result<(), ConsoleError> {
        self.inner.progress_finish()
    }
    pub fn title(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.inner.title(message)
    }
    pub fn section(&mut self, message: &str) -> Result<(), ConsoleError> {
        self.inner.section(message)
    }
    pub fn listing<I, S>(&mut self, elements: I) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.inner.listing(elements)
    }
    pub fn text<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.inner.text(messages)
    }
    pub fn block<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
        options: BlockOptions<'_>,
    ) -> Result<(), ConsoleError> {
        self.inner.block(messages, options)
    }
    pub fn comment<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.inner.comment(messages)
    }
    pub fn success<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.inner.success(messages)
    }
    pub fn error<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.inner.error(messages)
    }
    pub fn warning<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.inner.warning(messages)
    }
    pub fn note<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.inner.note(messages)
    }
    pub fn info<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.inner.info(messages)
    }
    pub fn caution<'m>(
        &mut self,
        messages: impl Into<OutputMessages<'m>>,
    ) -> Result<(), ConsoleError> {
        self.inner.caution(messages)
    }
    pub fn create_table(&mut self) -> Result<crate::console::Table<'_>, ConsoleError> {
        self.inner.create_table()
    }
    pub fn table<I, R>(
        &mut self,
        headers: impl Into<crate::console::TableHeaders>,
        rows: I,
    ) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = R>,
        R: Into<crate::console::TableRow>,
    {
        self.inner.table(headers, rows)
    }
    pub fn horizontal_table<I, R>(
        &mut self,
        headers: impl Into<crate::console::TableHeaders>,
        rows: I,
    ) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = R>,
        R: Into<crate::console::TableRow>,
    {
        self.inner.horizontal_table(headers, rows)
    }
    pub fn definition_list<I, T>(&mut self, list: I) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = T>,
        T: Into<crate::console::DefinitionListItem>,
    {
        self.inner.definition_list(list)
    }
    pub async fn ask(&mut self, label: &str) -> Result<Option<String>, ConsoleError> {
        self.inner.ask(label).await
    }
    pub async fn ask_with(
        &mut self,
        label: &str,
        default: Option<&str>,
    ) -> Result<Option<String>, ConsoleError> {
        self.inner.ask_with(label, default).await
    }
    pub async fn confirm(&mut self, label: &str) -> Result<bool, ConsoleError> {
        self.inner.confirm(label).await
    }
    pub async fn confirm_with(&mut self, label: &str, default: bool) -> Result<bool, ConsoleError> {
        self.inner.confirm_with(label, default).await
    }
    pub async fn ask_with_validator(
        &mut self,
        label: &str,
        default: Option<&str>,
        validator: Option<QuestionCallback>,
    ) -> Result<Value, ConsoleError> {
        self.inner
            .ask_with_validator(label, default, validator)
            .await
    }
    pub async fn ask_hidden(&mut self, label: &str) -> Result<Option<String>, ConsoleError> {
        self.inner.ask_hidden(label).await
    }
    pub async fn ask_hidden_with(
        &mut self,
        label: &str,
        validator: Option<QuestionCallback>,
    ) -> Result<Value, ConsoleError> {
        self.inner.ask_hidden_with(label, validator).await
    }
    pub async fn ask_with_completion(
        &mut self,
        label: &str,
        choices: CompletionCallback,
        default: Option<&str>,
    ) -> Result<Option<String>, ConsoleError> {
        self.inner
            .ask_with_completion(label, choices, default)
            .await
    }
    pub async fn choice<T: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = T>,
    ) -> Result<Value, ConsoleError> {
        self.inner.choice(label, choices).await
    }
    pub async fn choice_with<T: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = T>,
        default: Option<Value>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        self.inner
            .choice_with(label, choices, default, multiple)
            .await
    }
    pub async fn choice_pairs_with<K: Into<ChoiceKey>, V: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = (K, V)>,
        default: Option<Value>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        self.inner
            .choice_pairs_with(label, choices, default, multiple)
            .await
    }
    pub fn create_progress_bar(&mut self, max: i64) -> Result<ProgressBar<'_>, ConsoleError> {
        self.inner.create_progress_bar(max)
    }
    pub fn progress_iterate<I: IntoIterator>(
        &mut self,
        items: I,
    ) -> ProgressStyleIterator<'_, 'output, I> {
        self.inner.progress_iterate(items)
    }
    pub fn progress_iterate_with_max<I: IntoIterator>(
        &mut self,
        items: I,
        max: Option<i64>,
    ) -> ProgressStyleIterator<'_, 'output, I> {
        self.inner.progress_iterate_with_max(items, max)
    }
}
impl crate::console::AsOutput for ConsoleStyle<'_> {
    fn as_output(&mut self) -> crate::console::OutputReference<'_> {
        self.inner.as_output()
    }
}
impl OutputStyle<'_> {
    /// 依据直接输出类型选择错误流；返回不提供换行计数的基础样式。
    pub fn get_error_style(&mut self) -> ConsoleStyle<'_> {
        ConsoleStyle::from_reference(self.input, self.output.for_error_style())
    }
}
impl StyleInterface for ConsoleStyle<'_> {
    fn title(&mut self, message: &str) -> Result<(), ConsoleError> {
        OutputStyle::title(&mut self.inner, message)
    }
    fn section(&mut self, message: &str) -> Result<(), ConsoleError> {
        OutputStyle::section(&mut self.inner, message)
    }
    fn listing(&mut self, elements: &[&str]) -> Result<(), ConsoleError> {
        OutputStyle::listing(&mut self.inner, elements.iter().copied())
    }
    fn text(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::text(&mut self.inner, message)
    }
    fn success(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::success(&mut self.inner, message)
    }
    fn error(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::error(&mut self.inner, message)
    }
    fn warning(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::warning(&mut self.inner, message)
    }
    fn note(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::note(&mut self.inner, message)
    }
    fn caution(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::caution(&mut self.inner, message)
    }
    fn table(&mut self, headers: TableHeaders, rows: Vec<TableRow>) -> Result<(), ConsoleError> {
        OutputStyle::table(&mut self.inner, headers, rows)
    }
    fn ask<'a>(
        &'a mut self,
        question: &'a str,
        default: Option<&'a str>,
        validator: Option<QuestionCallback>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(OutputStyle::ask_with_validator(
            &mut self.inner,
            question,
            default,
            validator,
        ))
    }
    fn ask_hidden<'a>(
        &'a mut self,
        question: &'a str,
        validator: Option<QuestionCallback>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(OutputStyle::ask_hidden_with(
            &mut self.inner,
            question,
            validator,
        ))
    }
    fn confirm<'a>(
        &'a mut self,
        question: &'a str,
        default: bool,
    ) -> ConsoleFuture<'a, Result<bool, ConsoleError>> {
        Box::pin(OutputStyle::confirm_with(
            &mut self.inner,
            question,
            default,
        ))
    }
    fn choice<'a>(
        &'a mut self,
        question: &'a str,
        choices: Vec<(ChoiceKey, Value)>,
        default: Option<Value>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(OutputStyle::choice_pairs_with(
            &mut self.inner,
            question,
            choices,
            default,
            false,
        ))
    }
    fn new_line(&mut self, count: usize) -> Result<(), ConsoleError> {
        OutputStyle::new_line(&mut self.inner, count)
    }
    fn progress_start(&mut self, max: i64) -> Result<(), ConsoleError> {
        OutputStyle::progress_start(&mut self.inner, max)
    }
    fn progress_advance(&mut self, steps: i64) -> Result<(), ConsoleError> {
        OutputStyle::progress_advance_by(&mut self.inner, steps)
    }
    fn progress_finish(&mut self) -> Result<(), ConsoleError> {
        OutputStyle::progress_finish(&mut self.inner)
    }
}

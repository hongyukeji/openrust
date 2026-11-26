pub use super::ConsoleStyle;
use super::{
    ChoiceKey, ConsoleError, ConsoleFuture, OutputMessages, OutputStyle, QuestionCallback,
    TableHeaders, TableRow,
};
use serde_json::Value;

/// 命令样式的公开接口；问答使用既有异步执行机制。
/// 可通过泛型或 `&mut dyn StyleInterface` 消费，不要求实现换行计数接口。
pub trait StyleInterface: Send {
    fn title(&mut self, message: &str) -> Result<(), ConsoleError>;
    fn section(&mut self, message: &str) -> Result<(), ConsoleError>;
    fn listing(&mut self, elements: &[&str]) -> Result<(), ConsoleError>;
    fn text(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError>;
    fn success(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError>;
    fn error(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError>;
    fn warning(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError>;
    fn note(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError>;
    fn caution(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError>;
    fn table(&mut self, headers: TableHeaders, rows: Vec<TableRow>) -> Result<(), ConsoleError>;
    fn ask<'a>(
        &'a mut self,
        question: &'a str,
        default: Option<&'a str>,
        validator: Option<QuestionCallback>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>>;
    fn ask_hidden<'a>(
        &'a mut self,
        question: &'a str,
        validator: Option<QuestionCallback>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>>;
    fn confirm<'a>(
        &'a mut self,
        question: &'a str,
        default: bool,
    ) -> ConsoleFuture<'a, Result<bool, ConsoleError>>;
    fn choice<'a>(
        &'a mut self,
        question: &'a str,
        choices: Vec<(ChoiceKey, Value)>,
        default: Option<Value>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>>;
    fn new_line(&mut self, count: usize) -> Result<(), ConsoleError>;
    fn progress_start(&mut self, max: i64) -> Result<(), ConsoleError>;
    fn progress_advance(&mut self, steps: i64) -> Result<(), ConsoleError>;
    fn progress_finish(&mut self) -> Result<(), ConsoleError>;
}

impl StyleInterface for OutputStyle<'_> {
    fn title(&mut self, message: &str) -> Result<(), ConsoleError> {
        OutputStyle::title(self, message)
    }
    fn section(&mut self, message: &str) -> Result<(), ConsoleError> {
        OutputStyle::section(self, message)
    }
    fn listing(&mut self, elements: &[&str]) -> Result<(), ConsoleError> {
        OutputStyle::listing(self, elements.iter().copied())
    }
    fn text(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::text(self, message)
    }
    fn success(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::success(self, message)
    }
    fn error(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::error(self, message)
    }
    fn warning(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::warning(self, message)
    }
    fn note(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::note(self, message)
    }
    fn caution(&mut self, message: OutputMessages<'_>) -> Result<(), ConsoleError> {
        OutputStyle::caution(self, message)
    }
    fn table(&mut self, headers: TableHeaders, rows: Vec<TableRow>) -> Result<(), ConsoleError> {
        OutputStyle::table(self, headers, rows)
    }
    fn ask<'a>(
        &'a mut self,
        question: &'a str,
        default: Option<&'a str>,
        validator: Option<QuestionCallback>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(OutputStyle::ask_with_validator(
            self, question, default, validator,
        ))
    }
    fn ask_hidden<'a>(
        &'a mut self,
        question: &'a str,
        validator: Option<QuestionCallback>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(OutputStyle::ask_hidden_with(self, question, validator))
    }
    fn confirm<'a>(
        &'a mut self,
        question: &'a str,
        default: bool,
    ) -> ConsoleFuture<'a, Result<bool, ConsoleError>> {
        Box::pin(OutputStyle::confirm_with(self, question, default))
    }
    fn choice<'a>(
        &'a mut self,
        question: &'a str,
        choices: Vec<(ChoiceKey, Value)>,
        default: Option<Value>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>> {
        Box::pin(OutputStyle::choice_pairs_with(
            self, question, choices, default, false,
        ))
    }
    fn new_line(&mut self, count: usize) -> Result<(), ConsoleError> {
        OutputStyle::new_line(self, count)
    }
    fn progress_start(&mut self, max: i64) -> Result<(), ConsoleError> {
        OutputStyle::progress_start(self, max)
    }
    fn progress_advance(&mut self, steps: i64) -> Result<(), ConsoleError> {
        OutputStyle::progress_advance_by(self, steps)
    }
    fn progress_finish(&mut self) -> Result<(), ConsoleError> {
        OutputStyle::progress_finish(self)
    }
}

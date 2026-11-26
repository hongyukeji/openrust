// 第三方许可见 LICENSE。
use super::mutators::{
    EnsureDynamicContentIsHighlighted, EnsureNoPunctuation, EnsureRelativePaths, Mutator,
};
use crate::console::{
    AsOutput, ConsoleError, OutputOptions, OutputReference, OutputVerbosity, Terminal,
};
use crate::support::interacts_with_time::wall_time;
use regex::Regex;
use serde_json::Value;
use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::LazyLock,
};

type RunTimeFormatter<'a> =
    Box<dyn FnMut(f64, Option<f64>) -> Result<String, ConsoleError> + Send + 'a>;
/// 在同一所选输出上运行同步任务；失败后先收尾显示，再传播原错误。
pub struct Task<'a> {
    output: OutputReference<'a>,
    run_time_formatter: Option<RunTimeFormatter<'a>>,
}
impl<'a> Task<'a> {
    pub fn new(output: &'a mut (impl AsOutput + ?Sized)) -> Self {
        Self {
            output: output.as_output(),
            run_time_formatter: None,
        }
    }
    /// 以组合替代耗时方法覆盖；None恢复默认格式器。
    pub fn set_run_time_formatter(&mut self, formatter: Option<RunTimeFormatter<'a>>) {
        self.run_time_formatter = formatter;
    }
    pub fn run_time_for_humans(
        &mut self,
        start: f64,
        end: Option<f64>,
    ) -> Result<String, ConsoleError> {
        if let Some(formatter) = &mut self.run_time_formatter {
            return formatter(start, end);
        }
        crate::support::interacts_with_time::run_time_for_humans(start, end).map_err(|error| {
            match error {
                crate::support::interacts_with_time::RunTimeError::Format => {
                    ConsoleError::new("任务耗时超出范围")
                }
                crate::support::interacts_with_time::RunTimeError::Integer(source) => {
                    ConsoleError::from_source(source)
                }
            }
        })
    }
    pub fn seconds_until(
        &self,
        delay: impl Into<crate::support::TimeDelay>,
    ) -> Result<i64, ConsoleError> {
        crate::support::InteractsWithTime::seconds_until(self, delay)
    }
    pub fn available_at(
        &self,
        delay: impl Into<crate::support::TimeDelay>,
    ) -> Result<i64, ConsoleError> {
        crate::support::InteractsWithTime::available_at(self, delay)
    }
    pub fn parse_date_interval(
        &self,
        delay: impl Into<crate::support::TimeDelay>,
    ) -> Result<crate::support::TimeDelay, ConsoleError> {
        crate::support::InteractsWithTime::parse_date_interval(self, delay)
    }
    pub fn current_time(&self) -> Result<i64, ConsoleError> {
        crate::support::InteractsWithTime::current_time(self)
    }

    pub fn render(&mut self, description: &str) -> Result<(), ConsoleError> {
        self.render_with(description, OutputVerbosity::Normal)
    }
    pub fn render_with(
        &mut self,
        description: &str,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.render_internal(
            description,
            None::<fn(&mut OutputReference<'_>) -> Result<Value, ConsoleError>>,
            options.into(),
        )
    }
    /// 显式借出同一输出，避免捕获可变流形成别名；仅整数2/3为失败/跳过。
    pub fn render_task<T: Into<Value>>(
        &mut self,
        description: &str,
        task: impl FnOnce(&mut OutputReference<'_>) -> Result<T, ConsoleError>,
    ) -> Result<(), ConsoleError> {
        self.render_task_with(description, task, OutputVerbosity::Normal)
    }
    pub fn render_task_with<T: Into<Value>>(
        &mut self,
        description: &str,
        task: impl FnOnce(&mut OutputReference<'_>) -> Result<T, ConsoleError>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        self.render_internal(
            description,
            Some(|output: &mut OutputReference<'_>| task(output).map(Into::into)),
            options.into(),
        )
    }
    fn render_internal<F>(
        &mut self,
        description: &str,
        task: Option<F>,
        options: OutputOptions,
    ) -> Result<(), ConsoleError>
    where
        F: FnOnce(&mut OutputReference<'_>) -> Result<Value, ConsoleError>,
    {
        let description = EnsureDynamicContentIsHighlighted.apply(description)?;
        let description = EnsureNoPunctuation.apply(&description)?;
        let description = EnsureRelativePaths.apply(&description)?;
        static STYLING: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"<[A-Za-z0-9_=#/;,:.&,%?]+>|\x1b\[[0-9]+m").expect("固定的任务样式表达式")
        });
        let length = STYLING.replace_all(&description, "").chars().count();
        self.output
            .write(format!("  {description} ").as_str(), false, options)?;
        let start = wall_time();
        let has_task = task.is_some();
        let result = catch_unwind(AssertUnwindSafe(|| {
            task.map_or(Ok(Value::from(1)), |task| task(&mut self.output))
        }));
        let runtime = if has_task {
            format!(" {}", self.run_time_for_humans(start, None)?)
        } else {
            String::new()
        };
        let width = i128::from(Terminal::new().get_width().min(150));
        let dots =
            usize::try_from((width - length as i128 - runtime.chars().count() as i128 - 10).max(0))
                .map_err(ConsoleError::from_source)?;
        self.output
            .write("<fg=gray>.</>".repeat(dots).as_str(), false, options)?;
        self.output
            .write(format!("<fg=gray>{runtime}</>").as_str(), false, options)?;
        let label = match &result {
            Ok(Ok(Value::Number(value))) if value.as_i64() == Some(2) => {
                " <fg=red;options=bold>FAIL</>"
            }
            Ok(Ok(Value::Number(value))) if value.as_i64() == Some(3) => {
                " <fg=yellow;options=bold>SKIPPED</>"
            }
            Ok(Ok(_)) => " <fg=green;options=bold>DONE</>",
            _ => " <fg=red;options=bold>FAIL</>",
        };
        self.output.writeln(label, options)?;
        match result {
            Ok(result) => result.map(|_| ()),
            Err(payload) => resume_unwind(payload),
        }
    }
}
impl crate::support::InteractsWithTime for Task<'_> {
    type Error = ConsoleError;
    fn run_time_for_humans(
        &mut self,
        start: f64,
        end: Option<f64>,
    ) -> Result<String, ConsoleError> {
        Task::run_time_for_humans(self, start, end)
    }
}

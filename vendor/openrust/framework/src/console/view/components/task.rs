// 第三方许可见 LICENSE。
use super::mutators::{
    EnsureDynamicContentIsHighlighted, EnsureNoPunctuation, EnsureRelativePaths, Mutator,
};
use crate::console::{
    AsOutput, ConsoleError, OutputOptions, OutputReference, OutputVerbosity, Terminal,
};
use regex::Regex;
use serde_json::Value;
use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
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
        let end = match end {
            Some(end) => end,
            None => wall_time(),
        };
        let milliseconds = (end - start) * 1000.0;
        if milliseconds.is_nan() {
            return Ok("nanms".into());
        }
        if milliseconds.is_infinite() {
            return Ok(if milliseconds.is_sign_positive() {
                "0s"
            } else {
                "infms"
            }
            .into());
        }
        if milliseconds <= 1000.0 {
            let formatted = crate::support::calendar::format_decimal_number(milliseconds, 2)
                .map_err(|_| ConsoleError::new("任务耗时超出范围"))?;
            let (integer, fraction) = formatted.split_once('.').expect("固定精度包含小数点");
            let (sign, digits) = integer
                .strip_prefix('-')
                .map_or(("", integer), |n| ("-", n));
            let mut result = String::from(sign);
            for (i, digit) in digits.chars().enumerate() {
                if i > 0 && (digits.len() - i).is_multiple_of(3) {
                    result.push(',');
                }
                result.push(digit);
            }
            return Ok(format!("{result}.{fraction}ms"));
        }
        // 短单位显示向下取整秒；月份按四周、年份按十二个月进位。
        let mut seconds = format!("{:.0}", (milliseconds / 1000.0).floor())
            .parse::<i64>()
            .map_err(ConsoleError::from_source)?;
        let mut parts = Vec::new();
        for (unit, divisor, plural) in [
            ("yr", 29_030_400, "yrs"),
            ("mo", 2_419_200, "mos"),
            ("w", 604_800, "w"),
            ("d", 86_400, "d"),
            ("h", 3600, "h"),
            ("m", 60, "m"),
            ("s", 1, "s"),
        ] {
            let amount = seconds / divisor;
            seconds %= divisor;
            if amount > 0 {
                parts.push(format!(
                    "{amount}{}",
                    if amount == 1 { unit } else { plural }
                ));
            }
        }
        Ok(parts.join(" "))
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
fn wall_time() -> f64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs_f64(),
        Err(error) => -error.duration().as_secs_f64(),
    }
}

use super::{ConsoleError, OutputReference, OutputStyle, OutputVerbosity};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock},
    time::{SystemTime, UNIX_EPOCH},
};
mod error;
pub use error::ConsoleProgressError;
mod definitions;
pub(crate) use definitions::Definitions as ProgressDefinitions;
mod callbacks;
mod output;
pub use output::ProgressOutput;
mod iterable;
mod iterator;
pub use iterable::{ProgressIterable, ProgressIterableMut};
pub use iterator::{ProgressIterator, ProgressStyleIterator};

/// 占位符回调借用当前进度条，允许读取和修改同一个实例。
pub type ProgressFormatter =
    Arc<dyn for<'a> Fn(&mut ProgressBar<'a>) -> Result<String, ConsoleError> + Send + Sync>;
/// 进度状态独立于输出借用，供手动进度条与样式便利入口共用。
pub(super) struct ProgressState {
    definitions: Option<Arc<definitions::Definitions>>,
    max: Option<i64>,
    step: i64,
    starting_step: i64,
    width: usize,
    step_width: usize,
    percent: f64,
    bar_char: Option<String>,
    empty_char: String,
    progress_char: String,
    internal_format: Option<String>,
    format: Option<String>,
    messages: BTreeMap<String, String>,
    placeholders: BTreeMap<String, ProgressFormatter>,
    redraw: Option<usize>,
    min_seconds: f64,
    max_seconds: f64,
    overwrite: bool,
    previous: Option<String>,
    writes: usize,
    last_write: f64,
    start_time: i64,
}
/// 当前命令输出上的进度条，不拥有进程级输出状态。
pub struct ProgressBar<'a> {
    output: OutputReference<'a>,
    state: ProgressState,
}
impl<'a> ProgressBar<'a> {
    pub fn new(
        output: &'a mut (impl super::AsOutput + ?Sized),
        max: i64,
    ) -> Result<Self, ConsoleError> {
        Self::new_with_redraw_interval(output, max, 1.0 / 25.0)
    }
    pub fn new_with_redraw_interval(
        output: &'a mut (impl super::AsOutput + ?Sized),
        max: i64,
        min_seconds: f64,
    ) -> Result<Self, ConsoleError> {
        let output = output.as_output();
        let decorated = output.progress_is_decorated();
        let mut bar = Self {
            output,
            state: ProgressState {
                definitions: definitions::current(false)?,
                max: None,
                step: 0,
                starting_step: 0,
                width: 28,
                step_width: 4,
                percent: 0.0,
                bar_char: None,
                empty_char: "-".into(),
                progress_char: ">".into(),
                internal_format: None,
                format: None,
                messages: BTreeMap::new(),
                placeholders: BTreeMap::new(),
                redraw: if min_seconds > 0.0 || !decorated {
                    None
                } else {
                    Some(1)
                },
                min_seconds: if min_seconds > 0.0 { min_seconds } else { 0.0 },
                max_seconds: 1.0,
                overwrite: decorated,
                previous: None,
                writes: 0,
                last_write: 0.0,
                start_time: now_seconds(),
            },
        };
        bar.set_max_steps(Some(max));
        Ok(bar)
    }
    pub fn start(&mut self) -> Result<(), ConsoleError> {
        self.start_at(None, 0)
    }
    pub fn start_at(&mut self, max: Option<i64>, step: i64) -> Result<(), ConsoleError> {
        self.state.start_time = now_seconds();
        self.state.step = step;
        self.state.starting_step = step;
        if step > 0 {
            self.set_progress(step)?;
        } else {
            self.state.percent = 0.0;
        }
        if let Some(max) = max {
            self.set_max_steps(Some(max));
        }
        self.display()
    }
    pub fn advance(&mut self) -> Result<(), ConsoleError> {
        self.advance_by(1)
    }
    pub fn advance_by(&mut self, steps: i64) -> Result<(), ConsoleError> {
        let step = self.state.step.checked_add(steps).ok_or_else(|| {
            ConsoleError::from_source(ConsoleProgressError::StepOverflow {
                current: self.state.step,
                advance: steps,
            })
        })?;
        self.set_progress(step)
    }
    pub fn set_progress(&mut self, mut step: i64) -> Result<(), ConsoleError> {
        if self.state.max.is_some_and(|max| max != 0 && step > max) {
            self.state.max = Some(step);
        } else if step < 0 {
            step = 0;
        }
        let frequency = self
            .state
            .redraw
            .map(|n| n as f64)
            .unwrap_or(self.state.max.unwrap_or(10) as f64 / 10.0);
        let previous = if frequency != 0.0 {
            crate::support::input_scalars::double_integer(self.state.step as f64 / frequency)
        } else {
            0
        };
        let current = if frequency != 0.0 {
            crate::support::input_scalars::double_integer(step as f64 / frequency)
        } else {
            0
        };
        self.state.step = step;
        self.state.percent = match self.state.max {
            None => 0.0,
            Some(0) => 1.0,
            Some(max) => step as f64 / max as f64,
        };
        if self.state.max == Some(step) {
            return self.display();
        }
        let interval = now() - self.state.last_write;
        if interval < self.state.min_seconds {
            return Ok(());
        }
        if previous != current || interval >= self.state.max_seconds {
            self.display()?;
        }
        Ok(())
    }
    pub fn set_max_steps(&mut self, max: Option<i64>) -> &mut Self {
        self.state.max = max.filter(|n| *n != 0).map(|n| n.max(0));
        self.state.format = None;
        self.state.step_width = self.state.max.map_or(4, |max| max.to_string().len());
        self
    }
    pub fn finish(&mut self) -> Result<(), ConsoleError> {
        if self.state.max.is_none() {
            self.state.max = Some(self.state.step);
        }
        if self.state.max == Some(self.state.step) && !self.state.overwrite {
            return Ok(());
        }
        self.set_progress(self.state.max.unwrap_or(self.state.step))
    }
    pub fn set_bar_width(&mut self, width: i64) -> &mut Self {
        self.state.width = usize::try_from(width.max(1)).unwrap_or(usize::MAX);
        self
    }
    pub fn get_bar_width(&self) -> usize {
        self.state.width
    }
    pub fn set_bar_character(&mut self, text: impl Into<String>) -> &mut Self {
        self.state.bar_char = Some(text.into());
        self
    }
    pub fn get_bar_character(&self) -> &str {
        self.state
            .bar_char
            .as_deref()
            .unwrap_or(if self.state.max.is_some() {
                "="
            } else {
                &self.state.empty_char
            })
    }
    pub fn set_empty_bar_character(&mut self, text: impl Into<String>) -> &mut Self {
        self.state.empty_char = text.into();
        self
    }
    pub fn get_empty_bar_character(&self) -> &str {
        &self.state.empty_char
    }
    pub fn set_progress_character(&mut self, text: impl Into<String>) -> &mut Self {
        self.state.progress_char = text.into();
        self
    }
    pub fn get_progress_character(&self) -> &str {
        &self.state.progress_char
    }
    pub fn get_progress(&self) -> i64 {
        self.state.step
    }
    pub fn get_max_steps(&self) -> i64 {
        self.state.max.unwrap_or(0)
    }
    pub fn get_progress_percent(&self) -> f64 {
        self.state.percent
    }
    pub fn get_start_time(&self) -> i64 {
        self.state.start_time
    }
    pub fn get_bar_offset(&self) -> f64 {
        match self.state.max {
            Some(_) => (self.state.percent * self.state.width as f64).floor(),
            None => {
                let step = if self.state.redraw.is_none() {
                    crate::support::input_scalars::double_integer(
                        (self.state.width as f64 / 15.0).min(5.0) * self.state.writes as f64,
                    )
                } else {
                    self.state.step
                };
                (step % self.state.width as i64) as f64
            }
        }
    }
    pub fn get_estimated(&self) -> f64 {
        if self.state.step == 0 || self.state.step == self.state.starting_step {
            return 0.0;
        }
        ((integer_difference(now_seconds(), self.state.start_time)
            / integer_difference(self.state.step, self.state.starting_step))
            * self.state.max.unwrap_or(0) as f64)
            .round()
    }
    pub fn get_remaining(&self) -> f64 {
        if self.state.step == 0 || self.state.step == self.state.starting_step {
            return 0.0;
        }
        ((integer_difference(now_seconds(), self.state.start_time)
            / integer_difference(self.state.step, self.state.starting_step))
            * integer_difference(self.state.max.unwrap_or(0), self.state.step))
        .round()
    }
    pub fn set_overwrite(&mut self, overwrite: bool) -> &mut Self {
        self.state.overwrite = overwrite;
        self
    }
    pub fn set_redraw_frequency(&mut self, frequency: Option<i64>) -> &mut Self {
        self.state.redraw = frequency.map(|n| usize::try_from(n.max(1)).unwrap_or(usize::MAX));
        self
    }
    pub fn min_seconds_between_redraws(&mut self, seconds: f64) -> &mut Self {
        self.state.min_seconds = seconds;
        self
    }
    pub fn max_seconds_between_redraws(&mut self, seconds: f64) -> &mut Self {
        self.state.max_seconds = seconds;
        self
    }
    pub fn set_format(&mut self, format: impl Into<String>) -> &mut Self {
        self.state.internal_format = Some(format.into());
        self.state.format = None;
        self
    }
    pub fn set_message(&mut self, message: impl Into<String>) -> &mut Self {
        self.set_message_named(message, "message")
    }
    pub fn set_message_named(&mut self, message: impl Into<String>, name: &str) -> &mut Self {
        self.state.messages.insert(name.to_owned(), message.into());
        self
    }
    pub fn get_message(&self) -> Option<&str> {
        self.get_message_named("message")
    }
    pub fn get_message_named(&self, name: &str) -> Option<&str> {
        self.state.messages.get(name).map(String::as_str)
    }
    pub fn set_placeholder_formatter(
        &mut self,
        name: &str,
        formatter: ProgressFormatter,
    ) -> &mut Self {
        self.state.placeholders.insert(name.to_owned(), formatter);
        self
    }
    pub fn get_placeholder_formatter(&self, name: &str) -> Option<ProgressFormatter> {
        if let Some(formatter) = self.state.placeholders.get(name) {
            return Some(formatter.clone());
        }
        if let Some(callback) = self
            .state
            .definitions
            .as_ref()
            .and_then(|d| d.get_placeholder(name))
        {
            return Some(callback);
        }
        default_placeholder(name)
    }

    pub fn set_format_definition(name: &str, format: &str) -> Result<(), ConsoleError> {
        definitions::current(true)?
            .expect("已验证应用定义存在")
            .set_format(name, format);
        Ok(())
    }
    pub fn get_format_definition(name: &str) -> Result<Option<String>, ConsoleError> {
        Ok(definitions::current(false)?
            .and_then(|d| d.get_format(name))
            .or_else(|| built_in_format(name).map(str::to_owned)))
    }
    pub fn set_placeholder_formatter_definition(
        name: &str,
        formatter: ProgressFormatter,
    ) -> Result<(), ConsoleError> {
        definitions::current(true)?
            .expect("已验证应用定义存在")
            .set_placeholder(name, formatter);
        Ok(())
    }
    pub fn get_placeholder_formatter_definition(
        name: &str,
    ) -> Result<Option<ProgressFormatter>, ConsoleError> {
        if let Some(formatter) = definitions::current(false)?.and_then(|d| d.get_placeholder(name))
        {
            return Ok(Some(formatter));
        }
        Ok(default_placeholder(name))
    }
    fn lookup_format(&self, name: &str) -> Option<String> {
        self.state
            .definitions
            .as_ref()
            .and_then(|d| d.get_format(name))
            .or_else(|| built_in_format(name).map(str::to_owned))
    }
    pub(super) fn into_state(self) -> ProgressState {
        self.state
    }
    pub(super) fn from_state(
        output: &'a mut (impl super::AsOutput + ?Sized),
        state: ProgressState,
    ) -> Self {
        Self {
            output: output.as_output(),
            state,
        }
    }
    pub fn display(&mut self) -> Result<(), ConsoleError> {
        if self.output.progress_verbosity() == OutputVerbosity::Quiet {
            return Ok(());
        }
        self.resolve_format();
        let line = self.build_line()?;
        self.write_message(line)
    }
    pub fn clear(&mut self) -> Result<(), ConsoleError> {
        if !self.state.overwrite {
            return Ok(());
        }
        self.resolve_format();
        self.write_message(String::new())
    }
    fn resolve_format(&mut self) {
        if self.state.format.is_some() {
            return;
        }
        let level = self.output.progress_verbosity();
        let name = self
            .state
            .internal_format
            .clone()
            .filter(|s| !s.is_empty() && s != "0")
            .unwrap_or_else(|| {
                match level {
                    OutputVerbosity::Verbose => "verbose",
                    OutputVerbosity::VeryVerbose => "very_verbose",
                    OutputVerbosity::Debug => "debug",
                    _ => "normal",
                }
                .to_owned()
            });
        let key = if self.get_max_steps() == 0
            && self.lookup_format(&(name.clone() + "_nomax")).is_some()
        {
            name.clone() + "_nomax"
        } else {
            name.clone()
        };
        self.state.format = Some(self.lookup_format(&key).unwrap_or(name));
    }
    fn build_line(&mut self) -> Result<String, ConsoleError> {
        let mut line = self.interpolate()?;
        let mut width = 0;
        for part in line.split('\n') {
            let plain = super::formatting::unformatted_text(
                self.output.progress_formatter(),
                part.trim_end_matches('\r'),
            )
            .map_err(ConsoleError::from_source)?;
            width = width.max(super::formatting::display_width(&plain));
        }
        let terminal = super::Terminal::new().get_width();
        if width as i64 > terminal {
            let delta = i64::try_from(self.state.width).map_err(ConsoleError::from_source)?
                - i64::try_from(width).map_err(ConsoleError::from_source)?;
            let target = match delta.checked_add(terminal) {
                Some(value) => value,
                None => {
                    let value = delta as f64 + terminal as f64;
                    if !(i64::MIN as f64..9_223_372_036_854_775_808.0).contains(&value) {
                        return Err(ConsoleError::from_source(
                            ConsoleProgressError::InvalidBarWidth(value),
                        ));
                    }
                    value as i64
                }
            };
            self.set_bar_width(target);
            line = self.interpolate()?;
        }
        Ok(line)
    }
    fn interpolate(&mut self) -> Result<String, ConsoleError> {
        static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"%([a-zA-Z\-_]+)(?::([^%]+))?%").expect("合法进度占位符表达式")
        });
        let format = self.state.format.clone().unwrap_or_default();
        let mut result = String::new();
        let mut offset = 0;
        for captures in TOKENS.captures_iter(&format) {
            let span = captures.get(0).unwrap();
            result.push_str(&format[offset..span.start()]);
            let value = if let Some(formatter) = self.get_placeholder_formatter(&captures[1]) {
                Some(formatter(self)?)
            } else {
                self.state.messages.get(&captures[1]).cloned()
            };
            if let Some(mut value) = value {
                if let Some(spec) = captures.get(2) {
                    value = super::formatting::format_template(
                        &format!("%{}", spec.as_str()),
                        &Value::String(value),
                    )
                    .map_err(|error| {
                        ConsoleError::from_source(ConsoleProgressError::InvalidFormat(
                            ConsoleError::from_source(error),
                        ))
                    })?;
                }
                result.push_str(&value);
            } else {
                result.push_str(span.as_str());
            }
            offset = span.end();
        }
        result.push_str(&format[offset..]);
        Ok(result)
    }
    fn placeholder(&mut self, name: &str) -> Result<String, ConsoleError> {
        Ok(match name {
            "current" => format!("{:>width$}", self.state.step, width = self.state.step_width),
            "max" => self.get_max_steps().to_string(),
            "percent" => (self.state.percent * 100.0).floor().to_string(),
            "elapsed" => format_time(integer_difference(now_seconds(), self.state.start_time)),
            "remaining" => format_time(self.get_remaining()),
            "estimated" => format_time(self.get_estimated()),
            "memory" => {
                let bytes = memory_stats::memory_stats()
                    .ok_or_else(|| {
                        ConsoleError::from_source(ConsoleProgressError::MemoryUnavailable)
                    })?
                    .physical_mem;
                if bytes >= 1 << 30 {
                    format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64)
                } else if bytes >= 1 << 20 {
                    format!("{:.1} MiB", bytes as f64 / (1u64 << 20) as f64)
                } else if bytes >= 1 << 10 {
                    format!("{} KiB", bytes / 1024)
                } else {
                    format!("{bytes} B")
                }
            }
            "bar" => {
                let offset = self.get_bar_offset();
                if offset < 0.0 {
                    return Err(ConsoleError::from_source(
                        ConsoleProgressError::InvalidBarOffset(offset),
                    ));
                }
                let complete = offset as usize;
                let mut text = self.get_bar_character().repeat(complete);
                if complete < self.state.width {
                    let progress = self.state.progress_char.clone();
                    let plain = super::formatting::unformatted_text(
                        self.output.progress_formatter(),
                        &progress,
                    )
                    .map_err(ConsoleError::from_source)?;
                    let count = super::formatting::grapheme_length(&plain)
                        .map_err(ConsoleError::from_source)?;
                    let empty =
                        self.state
                            .width
                            .checked_sub(complete + count)
                            .ok_or_else(|| {
                                ConsoleError::from_source(
                                    ConsoleProgressError::ProgressCharacterTooWide,
                                )
                            })?;
                    text.push_str(&progress);
                    text.push_str(&self.state.empty_char.repeat(empty));
                }
                text
            }
            _ => String::new(),
        })
    }
    fn write_message(&mut self, mut message: String) -> Result<(), ConsoleError> {
        if self.state.previous.as_ref() == Some(&message) {
            return Ok(());
        }
        let original = message.clone();
        if self.state.overwrite {
            if let Some(previous) = &self.state.previous {
                if self.output.section.is_some() {
                    let parts: Vec<_> = previous.split('\n').collect();
                    let mut count =
                        i64::try_from(parts.len()).map_err(ConsoleError::from_source)?;
                    let last = super::formatting::unformatted_text(
                        self.output.progress_formatter(),
                        parts.last().copied().unwrap_or(""),
                    )
                    .map_err(ConsoleError::from_source)?;
                    if last.is_empty() {
                        count -= 1;
                    }
                    let width = super::Terminal::new().get_width();
                    for part in parts {
                        let plain = super::formatting::unformatted_text(
                            self.output.progress_formatter(),
                            part,
                        )
                        .map_err(ConsoleError::from_source)?;
                        let length = i64::try_from(super::formatting::display_width(&plain))
                            .map_err(ConsoleError::from_source)?;
                        if length > width {
                            if width == 0 {
                                return Err(ConsoleError::new("终端宽度为零，无法清除分区进度条"));
                            }
                            count = count
                                .checked_add((length as f64 / width as f64).floor() as i64)
                                .ok_or_else(|| ConsoleError::new("进度条清除行数超出可表示范围"))?;
                        }
                    }
                    self.output.clear_section(count)?;
                } else {
                    let mut cursor = String::new();
                    for _ in 0..previous.matches('\n').count() {
                        cursor.push_str("\x1b[1G\x1b[2K\x1b[1A");
                    }
                    cursor.push_str("\x1b[1G\x1b[2K");
                    self.output.emit_progress_message(
                        cursor.as_str(),
                        false,
                        super::OutputFormat::Raw,
                        super::OutputVerbosity::Normal,
                    )?;
                }
            }
        } else if self.state.step > 0 {
            message.insert(0, '\n');
        }
        self.state.previous = Some(original);
        self.state.last_write = now();
        self.state.writes += 1;
        self.output.emit_progress(message.as_str())
    }
}
fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or_else(
        |error| -error.duration().as_secs_f64(),
        |time| time.as_secs_f64(),
    )
}
fn built_in_format(name: &str) -> Option<&'static str> {
    Some(match name {
        "normal" => " %current%/%max% [%bar%] %percent:3s%%",
        "normal_nomax" => " %current% [%bar%]",
        "verbose" => " %current%/%max% [%bar%] %percent:3s%% %elapsed:6s%",
        "verbose_nomax" | "very_verbose_nomax" => " %current% [%bar%] %elapsed:6s%",
        "very_verbose" => " %current%/%max% [%bar%] %percent:3s%% %elapsed:6s%/%estimated:-6s%",
        "debug" => {
            " %current%/%max% [%bar%] %percent:3s%% %elapsed:6s%/%estimated:-6s% %memory:6s%"
        }
        "debug_nomax" => " %current% [%bar%] %elapsed:6s% %memory:6s%",
        _ => return None,
    })
}
fn format_time(seconds: f64) -> String {
    let mut milliseconds = crate::support::input_scalars::double_integer(seconds * 1000.0);
    if milliseconds == 0 {
        return "< 1 ms".into();
    }
    let formats = [
        (1, "ms"),
        (1000, "s"),
        (60000, "min"),
        (3600000, "h"),
        (86400000, "d"),
    ];
    let mut values = BTreeMap::new();
    for (index, (unit, label)) in formats.iter().enumerate() {
        let part = formats
            .get(index + 1)
            .map_or(milliseconds, |(next, _)| milliseconds % next);
        if index >= 2 {
            values.remove(&(index - 2));
        }
        if part == 0 {
            continue;
        }
        values.insert(
            index,
            format!(
                "{} {label}",
                crate::support::input_scalars::double_string(part as f64 / *unit as f64)
            ),
        );
        if part == milliseconds {
            break;
        }
        milliseconds -= part;
    }
    values.into_values().rev().collect::<Vec<_>>().join(", ")
}
impl<'o> OutputStyle<'o> {
    pub fn create_progress_bar(&mut self, max: i64) -> Result<ProgressBar<'_>, ConsoleError> {
        let mut bar = ProgressBar::new(self.get_output(), max)?;
        if !cfg!(windows) || std::env::var("TERM_PROGRAM").as_deref() == Ok("Hyper") {
            bar.set_empty_bar_character("░")
                .set_progress_character("")
                .set_bar_character("▓");
        }
        Ok(bar)
    }
}

impl super::CommandIo<'_> {
    pub fn with_progress_bar<T>(
        &mut self,
        steps: i64,
        callback: impl FnOnce(&mut ProgressBar<'_>) -> Result<T, ConsoleError>,
    ) -> Result<(), ConsoleError> {
        let mut bar = self.get_output().create_progress_bar(steps)?;
        bar.start()?;
        callback(&mut bar)?;
        bar.finish()
    }
    pub fn with_progress_bar_iter<I: ProgressIterable, T>(
        &mut self,
        items: I,
        mut callback: impl FnMut(
            &I::Item,
            &mut ProgressBar<'_>,
            crate::support::ArrayKey,
        ) -> Result<T, ConsoleError>,
    ) -> Result<I, ConsoleError> {
        let count =
            i64::try_from(items.progress_iter().len()).map_err(ConsoleError::from_source)?;
        let mut bar = self.get_output().create_progress_bar(count)?;
        bar.start()?;
        for (key, value) in items.progress_iter() {
            callback(value, &mut bar, key)?;
            bar.advance()?;
        }
        bar.finish()?;
        Ok(items)
    }
}

fn default_placeholder(name: &str) -> Option<ProgressFormatter> {
    static DEFAULTS: LazyLock<BTreeMap<&'static str, ProgressFormatter>> = LazyLock::new(|| {
        [
            "bar",
            "elapsed",
            "remaining",
            "estimated",
            "memory",
            "current",
            "max",
            "percent",
        ]
        .into_iter()
        .map(|name| {
            let callback: ProgressFormatter = Arc::new(move |bar| bar.placeholder(name));
            (name, callback)
        })
        .collect()
    });
    DEFAULTS.get(name).cloned()
}

fn now_seconds() -> i64 {
    now().floor() as i64
}
/// 整数差溢出时才转浮点，保留相邻大整数的精度与恢复起点的安全性。
fn integer_difference(left: i64, right: i64) -> f64 {
    left.checked_sub(right)
        .map_or_else(|| left as f64 - right as f64, |value| value as f64)
}

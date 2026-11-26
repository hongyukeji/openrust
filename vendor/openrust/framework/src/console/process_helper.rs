// 许可见 LICENSE.symfony。
use super::{
    AsOutput, ConsoleError, DebugFormatterHelper, Helper, HelperInterface, HelperSet,
    OutputOptions, OutputVerbosity,
};
use crate::process::{Command, OutputStream, PendingProcess, ProcessResult};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

type OutputCallback<'a> = dyn FnMut(OutputStream, &[u8]) + Send + 'a;

static NEXT_SESSION: AtomicU64 = AtomicU64::new(0);

/// 复用原生进程执行器，按所选控制台输出的详细级别显示诊断。
#[derive(Debug, Default)]
pub struct ProcessHelper(Helper);

impl ProcessHelper {
    pub const fn new() -> Self {
        Self(Helper::new())
    }

    pub const fn get_name(&self) -> &'static str {
        "process"
    }

    pub fn set_helper_set(&self, set: Option<&HelperSet>) {
        self.0.set_helper_set(set);
    }

    pub fn get_helper_set(&self) -> Option<HelperSet> {
        self.0.get_helper_set()
    }

    pub async fn run(
        &self,
        output: &mut (impl AsOutput + ?Sized),
        command: impl Into<PendingProcess>,
    ) -> Result<ProcessResult, ConsoleError> {
        self.run_with(output, command, None, None, OutputVerbosity::VeryVerbose)
            .await
    }

    pub async fn run_with(
        &self,
        output: &mut (impl AsOutput + ?Sized),
        command: impl Into<PendingProcess>,
        error_message: Option<&str>,
        callback: Option<&mut OutputCallback<'_>>,
        verbosity: OutputVerbosity,
    ) -> Result<ProcessResult, ConsoleError> {
        let mut selected = output.as_output();
        let mut output = selected.for_error_style();
        let formatter = self.debug_formatter()?;
        let command = command.into();
        let diagnose = output.get_verbosity() >= verbosity;
        let debug = output.is_debug();
        let mut callback = callback;
        let session = (diagnose || debug).then(|| Session {
            id: format!("process:{}", NEXT_SESSION.fetch_add(1, Ordering::Relaxed)),
            formatter: &formatter,
        });
        if let Some(session) = &session {
            let start = formatter.start(
                &session.id,
                &escape_process_text(&command_line(command.command())),
            );
            if diagnose {
                output.write(start.as_str(), false, OutputOptions::default())?;
            }
        }

        let capture = command.captures_output();
        let process = command.start().map_err(ConsoleError::from_source)?;
        let result = if capture && (debug || callback.is_some()) {
            let mut output_error = None;
            let result = process
                .wait_with_output(|stream, bytes| {
                    if output_error.is_some() {
                        return;
                    }
                    if let Some(session) = session.as_ref().filter(|_| debug) {
                        let message = debug_message(&formatter, &session.id, stream, bytes);
                        if let Err(error) = message.and_then(|message| {
                            output.write(message.as_str(), false, OutputOptions::default())
                        }) {
                            output_error = Some(error);
                            return;
                        }
                    }
                    if let Some(callback) = &mut callback {
                        callback(stream, bytes);
                    }
                })
                .await;
            if let Some(error) = output_error {
                return Err(error);
            }
            result
        } else {
            process.wait().await
        }
        .map_err(ConsoleError::from_source)?;

        if let Some(session) = session.as_ref().filter(|_| diagnose) {
            let message = if result.successful() {
                "Command ran successfully".to_owned()
            } else {
                format!("{} Command did not run successfully", result.exit_code())
            };
            let stop = formatter.stop(&session.id, &message, result.successful())?;
            output.write(stop.as_str(), false, OutputOptions::default())?;
        }
        if result.failed()
            && let Some(message) = error_message
        {
            let message = format!("<error>{}</error>", escape_process_text(message));
            output.writeln(message.as_str(), OutputOptions::default())?;
        }
        Ok(result)
    }

    pub async fn must_run(
        &self,
        output: &mut (impl AsOutput + ?Sized),
        command: impl Into<PendingProcess>,
    ) -> Result<ProcessResult, ConsoleError> {
        self.run(output, command)
            .await?
            .throw()
            .map_err(ConsoleError::from_source)
    }

    pub async fn must_run_with(
        &self,
        output: &mut (impl AsOutput + ?Sized),
        command: impl Into<PendingProcess>,
        error_message: Option<&str>,
        callback: Option<&mut OutputCallback<'_>>,
        verbosity: OutputVerbosity,
    ) -> Result<ProcessResult, ConsoleError> {
        self.run_with(output, command, error_message, callback, verbosity)
            .await?
            .throw()
            .map_err(ConsoleError::from_source)
    }

    /// 以已开始的调试会话包装回调。先写诊断，再把原始字节交给调用方。
    pub fn wrap_callback<'a>(
        &self,
        output: &'a mut (impl AsOutput + ?Sized),
        id: &'a str,
        mut callback: Option<&'a mut OutputCallback<'a>>,
    ) -> Result<impl FnMut(OutputStream, &[u8]) -> Result<(), ConsoleError> + 'a, ConsoleError>
    {
        let formatter = self.debug_formatter()?;
        let mut selected = output.as_output();
        Ok(move |stream, bytes: &[u8]| {
            let message = debug_message(&formatter, id, stream, bytes)?;
            selected
                .for_error_style()
                .write(message.as_str(), false, OutputOptions::default())?;
            if let Some(callback) = &mut callback {
                callback(stream, bytes);
            }
            Ok(())
        })
    }

    fn debug_formatter(&self) -> Result<Arc<DebugFormatterHelper>, ConsoleError> {
        let set = self
            .get_helper_set()
            .ok_or_else(|| ConsoleError::new("进程助手尚未加入控制台助手集合"))?;
        let helper: Arc<dyn std::any::Any + Send + Sync> = set
            .get("debug_formatter")
            .map_err(ConsoleError::from_source)?;
        helper
            .downcast()
            .map_err(|_| ConsoleError::new("控制台调试助手类型不匹配"))
    }
}

impl HelperInterface for ProcessHelper {
    fn get_name(&self) -> &str {
        self.get_name()
    }

    fn set_helper_set(&self, set: Option<&HelperSet>) {
        self.set_helper_set(set);
    }

    fn get_helper_set(&self) -> Option<HelperSet> {
        self.get_helper_set()
    }
}

crate::console::helper::impl_helper_tools!(ProcessHelper);

struct Session<'a> {
    id: String,
    formatter: &'a DebugFormatterHelper,
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        self.formatter.forget(&self.id);
    }
}

fn debug_message(
    formatter: &DebugFormatterHelper,
    id: &str,
    stream: OutputStream,
    bytes: &[u8],
) -> Result<String, ConsoleError> {
    formatter.progress(
        id,
        &escape_process_text(&String::from_utf8_lossy(bytes)),
        stream == OutputStream::Stderr,
    )
}

fn escape_process_text(value: &str) -> String {
    value.replace('<', "\\<")
}

// 仅用于诊断。实际启动保留原生参数，诊断文本不会作为命令再次执行。
fn command_line(command: &Command) -> String {
    match command {
        Command::Shell(script) => script.to_string_lossy().into_owned(),
        Command::Arguments(args) => args
            .iter()
            .map(|arg| display_argument(&arg.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(unix)]
fn display_argument(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(windows)]
fn display_argument(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_owned();
    }
    let value = value.replace('\0', "?");
    if !value
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || b"()%!^\"<>&|".contains(&byte))
    {
        return value;
    }
    let body = value.trim_end_matches('\\');
    let mut quoted = String::from("\"");
    for char in body.chars() {
        match char {
            '"' => quoted.push_str("\"\""),
            '^' => quoted.push_str("\"^^\""),
            '%' => quoted.push_str("\"^%\""),
            '!' => quoted.push_str("\"^!\""),
            '\n' => quoted.push_str("!LF!"),
            char => quoted.push(char),
        }
    }
    let trailing = &value[body.len()..];
    quoted.push_str(trailing);
    quoted.push_str(trailing);
    quoted.push('"');
    quoted
}

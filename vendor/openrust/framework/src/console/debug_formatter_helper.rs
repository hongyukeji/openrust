// 许可见 LICENSE.symfony。
use super::{ConsoleError, Helper, HelperInterface, HelperSet};
use std::collections::HashMap;

const COLORS: [&str; 9] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white", "default",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Out,
    Error,
}

#[derive(Debug)]
struct Session {
    color: &'static str,
    stream: Option<Stream>,
}

#[derive(Debug, Default)]
struct State {
    sessions: HashMap<String, Session>,
    next_color: usize,
}

/// 按执行标识保存输出流切换状态，并为调试消息生成控制台标签。
#[derive(Debug, Default)]
pub struct DebugFormatterHelper {
    helper: Helper,
    state: parking_lot::Mutex<State>,
}

impl DebugFormatterHelper {
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) fn forget(&self, id: &str) {
        self.state.lock().sessions.remove(id);
    }

    pub const fn get_name(&self) -> &'static str {
        "debug_formatter"
    }

    pub fn set_helper_set(&self, set: Option<&HelperSet>) {
        self.helper.set_helper_set(set);
    }

    pub fn get_helper_set(&self) -> Option<HelperSet> {
        self.helper.get_helper_set()
    }

    pub fn start(&self, id: &str, message: &str) -> String {
        self.start_with(id, message, "RUN")
    }

    pub fn start_with(&self, id: &str, message: &str, prefix: &str) -> String {
        let color = {
            let mut state = self.state.lock();
            let color = COLORS[state.next_color];
            state.next_color = (state.next_color + 1) % COLORS.len();
            state.sessions.insert(
                id.to_owned(),
                Session {
                    color,
                    stream: None,
                },
            );
            color
        };
        format!("<bg={color}> </><bg=blue;fg=white> {prefix} </> <fg=blue>{message}</>\n")
    }

    pub fn progress(&self, id: &str, buffer: &str, error: bool) -> Result<String, ConsoleError> {
        self.progress_with(id, buffer, error, "OUT", "ERR")
    }

    pub fn progress_with(
        &self,
        id: &str,
        buffer: &str,
        error: bool,
        prefix: &str,
        error_prefix: &str,
    ) -> Result<String, ConsoleError> {
        let (stream, background, prefix) = if error {
            (Stream::Error, "red", error_prefix)
        } else {
            (Stream::Out, "green", prefix)
        };
        let (border, previous) = {
            let mut state = self.state.lock();
            let session = state
                .sessions
                .get_mut(id)
                .ok_or_else(|| missing_session(id))?;
            (session.color, session.stream.replace(stream))
        };
        let label = format!("<bg={border}> </><bg={background};fg=white> {prefix} </> ");
        let mut rendered = String::new();
        if previous.is_some_and(|previous| previous != stream) {
            rendered.push('\n');
        }
        if previous != Some(stream) {
            rendered.push_str(&label);
        }
        rendered.push_str(&buffer.replace('\n', &format!("\n{label}")));
        Ok(rendered)
    }

    pub fn stop(&self, id: &str, message: &str, successful: bool) -> Result<String, ConsoleError> {
        self.stop_with(id, message, successful, "RES")
    }

    pub fn stop_with(
        &self,
        id: &str,
        message: &str,
        successful: bool,
        prefix: &str,
    ) -> Result<String, ConsoleError> {
        let (border, has_stream) = {
            let mut state = self.state.lock();
            let session = state
                .sessions
                .get_mut(id)
                .ok_or_else(|| missing_session(id))?;
            let has_stream = session.stream.is_some();
            if !successful {
                session.stream = None;
            }
            (session.color, has_stream)
        };
        let separator = if has_stream { "\n" } else { "" };
        let color = if successful { "green" } else { "red" };
        Ok(format!(
            "{separator}<bg={border}> </><bg={color};fg=white> {prefix} </> <fg={color}>{message}</>\n"
        ))
    }
}

fn missing_session(id: &str) -> ConsoleError {
    ConsoleError::new(format!("控制台调试输出尚未开始：{id}"))
}

impl HelperInterface for DebugFormatterHelper {
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

crate::console::helper::impl_helper_tools!(DebugFormatterHelper);

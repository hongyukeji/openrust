use super::AnsiColorMode;
use std::sync::{Mutex, OnceLock};
static COLOR_MODE: Mutex<Option<AnsiColorMode>> = Mutex::new(None);
static DIMENSIONS: Mutex<Option<(i64, i64)>> = Mutex::new(None);
static STTY: OnceLock<bool> = OnceLock::new();
/// 进程终端的颜色能力与尺寸；环境尺寸按每次调用读取。
#[derive(Clone, Copy, Debug, Default)]
pub struct Terminal;
impl Terminal {
    pub const DEFAULT_COLOR_MODE: AnsiColorMode = AnsiColorMode::Ansi4;
    pub fn new() -> Self {
        Self
    }
    pub fn get_color_mode() -> AnsiColorMode {
        let mut current = COLOR_MODE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current.get_or_insert_with(|| {
            for key in ["COLORTERM", "TERM"] {
                if let Some(value) = std::env::var_os(key) {
                    let value = value.to_string_lossy().to_ascii_lowercase();
                    if value.contains("truecolor") {
                        return AnsiColorMode::Ansi24;
                    }
                    if value.contains("256color") {
                        return AnsiColorMode::Ansi8;
                    }
                }
            }
            Self::DEFAULT_COLOR_MODE
        })
    }
    pub fn set_color_mode(mode: Option<AnsiColorMode>) {
        *COLOR_MODE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = mode;
    }
    pub fn get_width(&self) -> i64 {
        dimension("COLUMNS", 0, 80)
    }
    pub fn get_height(&self) -> i64 {
        dimension("LINES", 1, 50)
    }
    /// 仅查询stty本身；尺寸读取使用原生终端接口，不启动外部进程。
    pub fn has_stty_available() -> bool {
        *STTY.get_or_init(|| {
            std::process::Command::new("stty")
                .stdin(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::null())
                .output()
                .is_ok_and(|out| !out.stdout.is_empty() && out.stdout != b"0")
        })
    }
}
fn dimension(key: &str, index: usize, fallback: i64) -> i64 {
    if let Some(value) = std::env::var_os(key) {
        return crate::support::input_scalars::integer_string(trim_environment(
            &value.to_string_lossy(),
        ));
    }
    let dimensions = {
        let mut cached = DIMENSIONS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cached.is_none() {
            *cached = native_dimensions();
        }
        *cached
    };
    let value = dimensions
        .map(|(width, height)| if index == 0 { width } else { height })
        .unwrap_or(0);
    if value == 0 { fallback } else { value }
}
#[cfg(unix)]
fn native_dimensions() -> Option<(i64, i64)> {
    // 读取标准输入的设备，重定向输出时仍保留当前终端的尺寸语义。
    rustix::termios::tcgetwinsize(std::io::stdin())
        .ok()
        .map(|size| (i64::from(size.ws_col), i64::from(size.ws_row)))
}
#[cfg(windows)]
fn native_dimensions() -> Option<(i64, i64)> {
    if let Some(value) = std::env::var_os("ANSICON") {
        static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
        let pattern = PATTERN.get_or_init(|| {
            regex::Regex::new(r"^(\d+)x(\d+)(?: \((\d+)x(\d+)\))?$").expect("合法控制台尺寸表达式")
        });
        if let Some(parts) = pattern.captures(trim_environment(&value.to_string_lossy())) {
            let parse = |index| crate::support::input_scalars::integer_string(&parts[index]);
            return Some((
                parse(1),
                if parts.get(4).is_some() {
                    parse(4)
                } else {
                    parse(2)
                },
            ));
        }
    }
    ::console::Term::stdout()
        .size_checked()
        .map(|(height, width)| (i64::from(width), i64::from(height)))
}
#[cfg(not(any(unix, windows)))]
fn native_dimensions() -> Option<(i64, i64)> {
    None
}

fn trim_environment(value: &str) -> &str {
    value.trim_matches([' ', '\t', '\n', '\r', '\0', '\u{b}'])
}

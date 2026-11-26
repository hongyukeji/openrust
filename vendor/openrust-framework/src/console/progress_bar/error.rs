use std::{error::Error, fmt};
#[derive(Debug)]
pub enum ConsoleProgressError {
    MissingDefinitions,
    InvalidFormat(super::ConsoleError),
    MissingProgressBar,
    StepOverflow { current: i64, advance: i64 },
    InvalidBarOffset(f64),
    InvalidBarWidth(f64),
    ProgressCharacterTooWide,
    MemoryUnavailable,
}
impl fmt::Display for ConsoleProgressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDefinitions => f.write_str("进度条定义需要当前应用作用域"),
            Self::InvalidFormat(error) => write!(f, "进度条格式模板无效：{error}"),
            Self::MissingProgressBar => f.write_str("进度条尚未启动"),
            Self::StepOverflow { current, advance } => {
                write!(f, "进度步数超出范围：{current} + {advance}")
            }
            Self::InvalidBarOffset(offset) => write!(f, "进度条位置无效：{offset}"),
            Self::InvalidBarWidth(width) => {
                write!(f, "终端宽度导致进度条尺寸超出整数范围：{width}")
            }
            Self::ProgressCharacterTooWide => f.write_str("进度字符超过进度条剩余宽度"),
            Self::MemoryUnavailable => f.write_str("无法读取进程驻留内存"),
        }
    }
}
impl Error for ConsoleProgressError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidFormat(error) => Some(error),
            _ => None,
        }
    }
}

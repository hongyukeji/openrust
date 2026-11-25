// 第三方许可见 ../LICENSE.symfony。
use std::{error::Error, fmt};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleTreeError {
    InvalidCurrent,
    InvalidKey,
    CannotRewind,
    Cycle(String),
}
impl fmt::Display for ConsoleTreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCurrent => f.write_str("树游标没有当前节点"),
            Self::InvalidKey => f.write_str("树游标没有有效的整数键"),
            Self::CannotRewind => f.write_str("已推进的树迭代器无法回到起点"),
            Self::Cycle(value) => write!(f, "树中检测到重复节点：\"{value}\""),
        }
    }
}
impl Error for ConsoleTreeError {}

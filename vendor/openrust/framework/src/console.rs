//! Console命令回调、输出与注册仓储。
//!
//! 异步控制台命令沿用应用启动和关闭生命周期。
//! 命令签名解析、交互式IO、Console Application/Kernel 等完整官方能力仍须独立对应验收。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod command;
pub(crate) mod command_registry;

pub use command::{Command, print};

use super::LogContext;
use std::{collections::HashMap, sync::Mutex};

/// 同一执行作用域内，分别保存频道上下文和未来频道使用的共享初值。
#[derive(Default)]
pub(super) struct ChannelContext(pub Mutex<Fields>);
#[derive(Clone, Default)]
pub(super) struct Fields {
    pub shared: LogContext,
    pub channels: HashMap<usize, (std::sync::Arc<()>, LogContext)>,
}

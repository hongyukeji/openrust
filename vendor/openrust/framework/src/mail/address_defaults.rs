use super::mailables::Address;
use std::{collections::HashMap, sync::Mutex};

#[derive(Clone, Default)]
pub(super) struct Defaults {
    pub from: Option<Address>,
    pub to: Option<Address>,
    pub reply_to: Option<Address>,
    pub return_path: Option<String>,
}

/// 全局地址指当前应用执行作用域，不能跨请求／队列任务复用动态覆盖。
#[derive(Default)]
pub(super) struct AddressOverrides(pub Mutex<HashMap<String, Defaults>>);

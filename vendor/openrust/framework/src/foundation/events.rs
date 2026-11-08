//! Foundation 内核生命周期事件。

use crate::events::Event;

/// HTTP 或控制台内核开始终止当前入口时派发的事件。
///
/// 两种入口使用同一事件类型。监听器在应用终止回调之前执行。
#[derive(Clone, Copy, Debug)]
pub struct Terminating;

impl Event for Terminating {
    const NAME: &'static str = r"Illuminate\Foundation\Events\Terminating";
}

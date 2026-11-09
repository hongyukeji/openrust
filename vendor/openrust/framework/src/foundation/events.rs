//! Foundation 内核生命周期事件。

use crate::events::Event;

/// 应用诊断路由收到请求后派发的事件。
///
/// 监听器可执行只读依赖检查。检查失败时返回错误，诊断路由将其报告为不健康。
#[derive(Clone, Copy, Debug)]
pub struct DiagnosingHealth;

impl Event for DiagnosingHealth {
    const NAME: &'static str = r"Illuminate\Foundation\Events\DiagnosingHealth";
}

/// HTTP 或控制台内核开始终止当前入口时派发的事件。
///
/// 两种入口使用同一事件类型。监听器在应用终止回调之前执行。
#[derive(Clone, Copy, Debug)]
pub struct Terminating;

impl Event for Terminating {
    const NAME: &'static str = r"Illuminate\Foundation\Events\Terminating";
}

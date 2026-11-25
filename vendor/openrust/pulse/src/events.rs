pub struct SharedBeat {
    pub time: i64,
    pub instance: String,
}
pub struct IsolatedBeat {
    pub time: i64,
}
impl openrust::contracts::events::Event for SharedBeat {
    fn name(&self) -> &str {
        "OpenRust\\Pulse\\Events\\SharedBeat"
    }
}
impl openrust::contracts::events::Event for IsolatedBeat {
    fn name(&self) -> &str {
        "OpenRust\\Pulse\\Events\\IsolatedBeat"
    }
}
pub struct ExceptionReported {
    pub class: String,
    pub location: Option<String>,
}
impl ExceptionReported {
    pub const NAME: &'static str = "OpenRust\\Pulse\\Events\\ExceptionReported";
}
impl openrust::contracts::events::Event for ExceptionReported {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// 通过 Pulse 显式报告的原生异常，保留错误对象和来源。
pub struct NativeExceptionReported {
    pub exception: openrust::contracts::debug::SharedException,
}
impl NativeExceptionReported {
    pub const NAME: &'static str = "OpenRust\\Pulse\\Events\\NativeExceptionReported";
}
impl openrust::contracts::events::Event for NativeExceptionReported {
    fn name(&self) -> &str {
        Self::NAME
    }
}

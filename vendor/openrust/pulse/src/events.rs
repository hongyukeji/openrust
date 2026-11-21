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
impl openrust::contracts::events::Event for ExceptionReported {
    fn name(&self) -> &str {
        "OpenRust\\Pulse\\Events\\ExceptionReported"
    }
}

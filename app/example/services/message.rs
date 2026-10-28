//! 示例问候内容，展示业务依赖如何通过同一容器替换。

pub(crate) struct ExampleMessage(&'static str);

impl ExampleMessage {
    pub(crate) fn new(message: &'static str) -> Self {
        Self(message)
    }

    pub(crate) fn text(&self) -> &'static str {
        self.0
    }
}

impl Default for ExampleMessage {
    fn default() -> Self {
        Self::new("Hello from app/example")
    }
}

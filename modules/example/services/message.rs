//! 示例问候内容，展示模块配置与同一容器中的业务依赖替换。

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ExampleConfig {
    pub(crate) greeting: String,
}

impl Default for ExampleConfig {
    fn default() -> Self {
        Self {
            greeting: "Hello from modules/example".to_owned(),
        }
    }
}

pub(crate) struct ExampleMessage(String);

impl ExampleMessage {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn text(&self) -> &str {
        &self.0
    }
}

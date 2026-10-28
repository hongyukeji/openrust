//! 业务直接使用现有 serde/chrono 依赖定义响应。

#[derive(serde::Serialize)]
pub(crate) struct ExampleInfo {
    pub(crate) message: &'static str,
    pub(crate) served_at: chrono::DateTime<chrono::Utc>,
}

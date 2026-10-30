//! 固定 Laravel Support `MessageBag` 与 `ViewErrorBag` 的公开契约回归。
#[allow(
    clippy::expect_used,
    reason = "固定上游契约 fixture 或 API 错误时立即中止测试"
)]
#[path = "support/message_bag.rs"]
mod message_bag;
#[allow(
    clippy::expect_used,
    reason = "固定上游契约 fixture 或 API 错误时立即中止测试"
)]
#[path = "support/view_error_bag.rs"]
mod view_error_bag;

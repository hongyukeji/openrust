//! 独立 consumer 的测试入口。

#[cfg(test)]
mod consumer_tests;
#[cfg(all(test, feature = "session"))]
mod session_extension;

fn main() {}

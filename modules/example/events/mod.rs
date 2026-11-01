//! 示例业务事件的监听器贡献；共用 descriptor 中的唯一事件 Provider。

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
pub(crate) mod article;

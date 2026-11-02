//! 仅由显式 `db seed` 调用的示例数据；不建立可登录的默认账户。

use openrust::{Db, Result};

#[cfg(feature = "auth")]
mod articles;
#[cfg(all(
    test,
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]
pub(crate) use articles::SEED_ARTICLE_IDS;

/// 在主库单事务填充隔离示例数据；重复执行不覆盖已有内容。
#[cfg(feature = "auth")]
pub async fn run(database: Db) -> Result<()> {
    articles::run(database).await
}

/// 保留种子命令的可调用入口；实际执行时明确拒绝缺少的业务领域。
#[cfg(not(feature = "auth"))]
pub fn run(_database: Db) -> std::future::Ready<Result<()>> {
    std::future::ready(Err(openrust::Error::custom(
        "example_seed_requires_auth",
        "示例文章种子需要启用认证能力",
    )))
}

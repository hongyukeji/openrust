//! 仅由显式 `db seed` 调用的示例数据；不建立可登录的默认账户。

use openrust::{Db, Result};

#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
mod articles;
#[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
#[cfg(test)]
pub(crate) use articles::SEED_ARTICLE_IDS;

/// 在主库单事务填充隔离示例数据；重复执行不覆盖已有内容。
pub async fn run(database: Db) -> Result<()> {
    #[cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]
    {
        articles::run(database).await
    }
    #[cfg(not(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql"))))]
    {
        let _ = database;
        Err(openrust::Error::custom(
            "example_seed_requires_auth",
            "示例文章种子需要启用认证和数据库能力",
        ))
    }
}

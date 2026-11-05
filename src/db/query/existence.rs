//! 当前查询的存在性终结操作。

use super::{
    Alias, ConnectionTrait, DbBackend, Error, Expr, Query, QueryBuilder, Result, SelectStatement,
};

impl QueryBuilder<'_> {
    /// 判断原查询是否存在行；保留列、条件、排序、分页与 UNION，不读取完整结果集。
    ///
    /// 使用显式事务或当前读连接；来源上下文与数据库错误原样返回。
    pub async fn exists(&self) -> Result<bool> {
        let statement = self.statement(&self.existence_select())?;
        let row = match self.transaction {
            Some(transaction) => transaction.query_one_raw(statement).await,
            None => self.db.read_connection()?.query_one_raw(statement).await,
        }
        .map_err(Error::internal)?;
        let Some(row) = row else {
            return Ok(false);
        };
        match self.db.backend()? {
            DbBackend::Postgres => row.try_get::<bool>("", "exists").map_err(Error::internal),
            DbBackend::MySql | DbBackend::Sqlite => row
                .try_get::<i64>("", "exists")
                .map(|value| value != 0)
                .map_err(Error::internal),
            _ => Err(super::unsupported_backend()),
        }
    }

    /// 判断原查询是否没有行；只对成功的 `exists` 结果取反，不吞掉查询错误。
    pub async fn doesnt_exist(&self) -> Result<bool> {
        self.exists().await.map(|exists| !exists)
    }

    fn existence_select(&self) -> SelectStatement {
        Query::select()
            .expr_as(Expr::exists(self.scoped_select()), Alias::new("exists"))
            .to_owned()
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "existence_tests.rs"]
mod tests;

#[cfg(all(test, any(feature = "db-mysql", feature = "db-postgres")))]
#[path = "existence_real_tests.rs"]
mod real_tests;

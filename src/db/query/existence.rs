//! 当前查询的存在性终结操作。

use super::{
    Alias, ConnectionTrait, DbBackend, Error, Expr, Query, QueryBuilder, Result, SelectStatement,
};

impl QueryBuilder<'_, '_> {
    /// 判断原查询是否存在行；执行原对象回调，并保留当前查询形状及接入时子绑定。
    pub async fn exists(&self) -> Result<bool> {
        let _lease = self.lease()?;
        self.apply_callbacks()?;
        let state = self.snapshot()?;
        let wrap = |bindings| -> Result<SelectStatement> {
            Ok(Query::select()
                .expr_as(
                    Expr::exists(state.scoped_select(bindings)?),
                    Alias::new("exists"),
                )
                .to_owned())
        };
        let statement = state.statement_pair(&wrap(false)?, &wrap(true)?)?;
        let row = match state.transaction {
            Some(transaction) => transaction.query_one_raw(statement).await,
            None => state.db.read_connection()?.query_one_raw(statement).await,
        }
        .map_err(Error::internal)?;
        let Some(row) = row else {
            return Ok(false);
        };
        match state.db.backend()? {
            DbBackend::Postgres => row.try_get::<bool>("", "exists").map_err(Error::internal),
            DbBackend::MySql | DbBackend::Sqlite => row
                .try_get::<i64>("", "exists")
                .map(|value| value != 0)
                .map_err(Error::internal),
            _ => Err(super::unsupported_backend()),
        }
    }
    /// 判断原查询是否没有行；只对成功的存在性结果取反。
    pub async fn doesnt_exist(&self) -> Result<bool> {
        self.exists().await.map(|exists| !exists)
    }

    #[cfg(all(test, feature = "db-sqlite"))]
    fn existence_select(&self) -> Result<SelectStatement> {
        Ok(Query::select()
            .expr_as(Expr::exists(self.prepared_select()?), Alias::new("exists"))
            .to_owned())
    }
}

#[cfg(all(test, any(feature = "db-mysql", feature = "db-postgres")))]
#[path = "existence_real_tests.rs"]
mod real_tests;
#[cfg(all(test, feature = "db-sqlite"))]
#[path = "existence_tests.rs"]
mod tests;

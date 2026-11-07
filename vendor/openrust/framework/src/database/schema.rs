//! 所选Db上的原始列名清单；不缓存，不实现完整Schema元数据或模型可填字段判断。
//!
//! 首查primary，SQLite第二标量查使用Db同一读路由。显式绑定的事务必须由同源Db签发。
//! SQLx SQLite在第二步仍选writer时复用一个物理lease；不自动BEGIN或复制ATTACH状态。
use super::ReadRoute;
use super::{Db, TransactionContext};
use crate::{Error, Result};
use sea_orm::DatabaseReadLease;
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, QueryResult};

mod grammar;
use grammar::{ColumnQuery, compile};

/// 对同一个Db及可选同源借用事务的Schema视图；初建视图没有I/O。
#[derive(Clone)]
pub struct SchemaBuilder<'tx> {
    db: Db,
    transaction: Option<&'tx DatabaseTransaction>,
}
impl Db {
    /// 创建所选连接的Schema视图；克隆同一个Db，不访问连接池。
    ///
    /// ```
    /// let db = openrust::database::Db::default();
    /// let _schema = db.schema(); // 未初始化也可建视图，实际查询才报告未就绪。
    /// ```
    pub fn schema(&self) -> SchemaBuilder<'static> {
        SchemaBuilder {
            db: self.clone(),
            transaction: None,
        }
    }
}
impl SchemaBuilder<'_> {
    /// 使用Db签发的同源事务；校验发生在任何SQL之前。
    ///
    /// 同源clone有效，另一个Db即使包装同pool也拒绝；这不是授权/租户或具体事务身份判断。
    pub fn using_transaction<'tx>(
        self,
        context: &TransactionContext<'tx>,
    ) -> Result<SchemaBuilder<'tx>> {
        if !context.is_from(&self.db) {
            return Err(parameter_error(
                "schema_transaction_source_mismatch",
                "Schema事务不属于所选Db",
            ));
        }
        Ok(SchemaBuilder {
            db: self.db,
            transaction: Some(context.transaction()),
        })
    }
    /// 返回固定catalog列序中的原始列名；缺表为空，SQL/取值错误原样失败。
    ///
    /// 引用最多两段，不trim或归一名称。目录值绑定比固定PHP字符串拼接更安全，特殊名的
    /// 错误形态可能不同。SQLite首查为空仍执行第二标量查；第二失败不发布首组结果。
    pub async fn get_column_listing(&self, reference: &str) -> Result<Vec<String>> {
        grammar::parse(reference)?;
        let query = compile(self.db.backend()?, &self.db.table_names, reference)?;
        let rows = if let Some(transaction) = self.transaction {
            let rows = transaction
                .query_all_raw(query.columns)
                .await
                .map_err(Error::internal)?;
            if let Some(definition) = query.definition {
                consume_definition(
                    transaction
                        .query_one_raw(definition)
                        .await
                        .map_err(Error::internal)?,
                )?;
            }
            rows
        } else {
            self.read_columns(query).await?
        };
        rows.into_iter()
            .map(|row| row.try_get("", "name").map_err(Error::internal))
            .collect()
    }
    async fn read_columns(&self, query: ColumnQuery) -> Result<Vec<QueryResult>> {
        let primary = self.db.primary_connection()?;
        let (rows, lease) = primary
            .query_all_with_read_lease(query.columns)
            .await
            .map_err(Error::internal)?;
        if let Some(lease) = lease {
            self.read_with_lease(lease, query.definition).await?;
        } else if let Some(definition) = query.definition {
            let reader = self.db.read_connection()?;
            consume_definition(
                reader
                    .query_one_raw(definition)
                    .await
                    .map_err(Error::internal)?,
            )?;
        }
        Ok(rows)
    }
    async fn read_with_lease(
        &self,
        lease: DatabaseReadLease,
        definition: Option<sea_orm::Statement>,
    ) -> Result<()> {
        let Some(definition) = definition else {
            return Ok(());
        };
        let route = {
            let mut lease = lease;
            match self.db.read_route()? {
                ReadRoute::Primary => {
                    return consume_definition(
                        lease
                            .query_one_raw(definition)
                            .await
                            .map_err(Error::internal)?,
                    );
                }
                route @ ReadRoute::Replica(_) => route,
            }
            // 先归还写连接，再取读副本连接，避免同一单连接池自等。
        };
        let reader = self.db.read_route_connection(route)?;
        consume_definition(
            reader
                .query_one_raw(definition)
                .await
                .map_err(Error::internal)?,
        )
    }
}
fn consume_definition(row: Option<QueryResult>) -> Result<()> {
    if let Some(row) = row {
        let _: Option<String> = row.try_get("", "sql").map_err(Error::internal)?;
    }
    Ok(())
}
fn parameter_error(code: &str, message: &str) -> Error {
    Error::custom_with_status(
        crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        code,
        message,
    )
}
#[cfg(all(test, feature = "db-sqlite"))]
mod sqlite_tests;
#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "db-mysql"))]
mod mysql_tests;

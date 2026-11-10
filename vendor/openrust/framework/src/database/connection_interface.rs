use super::{DatabaseFuture, DatabaseResult, Row, Value, query, schema};

/// Builds queries and executes statements on the selected connection.
pub trait ConnectionInterface: Send {
    fn table(&mut self, table: &str, alias: Option<&str>) -> query::Builder<'_>;
    fn get_schema_builder(&mut self) -> schema::Builder<'_>;
    fn get_database_name(&self) -> &str;

    fn select<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [Value],
        use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Row>>>;

    fn select_one<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [Value],
        use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<Row>>>;

    fn scalar<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [Value],
        use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Value>>;

    fn insert<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [Value],
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;

    fn update<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [Value],
    ) -> DatabaseFuture<'a, DatabaseResult<u64>>;

    fn delete<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [Value],
    ) -> DatabaseFuture<'a, DatabaseResult<u64>>;

    fn statement<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [Value],
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;

    fn affecting_statement<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: &'a [Value],
    ) -> DatabaseFuture<'a, DatabaseResult<u64>>;

    fn unprepared<'a>(&'a mut self, sql: &'a str) -> DatabaseFuture<'a, DatabaseResult<bool>>;

    fn begin_transaction(&mut self) -> DatabaseFuture<'_, DatabaseResult<()>>;
    fn commit(&mut self) -> DatabaseFuture<'_, DatabaseResult<()>>;
    fn roll_back(&mut self, to_level: Option<u32>) -> DatabaseFuture<'_, DatabaseResult<()>>;
    fn transaction_level(&self) -> u32;
}

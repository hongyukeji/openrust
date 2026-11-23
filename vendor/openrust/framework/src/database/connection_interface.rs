use std::borrow::Cow;

use super::{
    Bindings, Connection, DatabaseCursor, DatabaseFuture, DatabaseResult, Row, Transaction, Value,
    query, schema,
};

/// SQL execution shared by pooled connections and pinned transactions.
///
/// A pooled connection honors the requested read role. A transaction always
/// uses its own session. Begin, commit and rollback belong to the concrete
/// connection and transaction APIs because the transaction owns that session.
pub trait ConnectionInterface: Send {
    fn table(&mut self, table: &str, alias: Option<&str>) -> query::Builder<'_>;
    fn get_schema_builder(&mut self) -> schema::Builder<'_>;
    fn get_database_name(&self) -> &str;

    fn raw(&self, sql: &str) -> query::Expression {
        query::Expression::new(sql)
    }

    fn select<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Row>>>;

    fn select_one<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<Row>>>;

    fn scalar<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Value>> {
        Box::pin(async move {
            self.select_one(sql, bindings, use_read)
                .await?
                .map_or(Ok(Value::Null), Row::scalar)
        })
    }

    fn cursor<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        use_read: bool,
    ) -> DatabaseCursor<'a>;

    fn insert<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        self.statement(sql, bindings)
    }

    fn update<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        self.affecting_statement(sql, bindings)
    }

    fn delete<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        self.affecting_statement(sql, bindings)
    }

    fn statement<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;

    fn affecting_statement<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>>;

    fn unprepared<'a>(&'a mut self, sql: &'a str) -> DatabaseFuture<'a, DatabaseResult<bool>>;
}

impl ConnectionInterface for Connection {
    fn table(&mut self, table: &str, alias: Option<&str>) -> query::Builder<'_> {
        Connection::table(self, &table_name(table, alias))
    }

    fn get_schema_builder(&mut self) -> schema::Builder<'_> {
        Connection::get_schema_builder(self)
    }

    fn get_database_name(&self) -> &str {
        Connection::get_database_name(self)
    }

    fn select<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Row>>> {
        if use_read {
            Box::pin(Connection::select(self, sql, bindings))
        } else {
            Box::pin(self.select_from_write_connection(sql, bindings))
        }
    }

    fn select_one<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<Row>>> {
        Box::pin(self.select_one_with_role(sql, bindings, use_read))
    }

    fn cursor<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        use_read: bool,
    ) -> DatabaseCursor<'a> {
        self.cursor_with_mode(sql, bindings, !use_read)
    }

    fn statement<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(Connection::statement(self, sql, bindings))
    }

    fn affecting_statement<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(Connection::affecting_statement(self, sql, bindings))
    }

    fn unprepared<'a>(&'a mut self, sql: &'a str) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(Connection::unprepared(self, sql))
    }
}

impl ConnectionInterface for Transaction {
    fn table(&mut self, table: &str, alias: Option<&str>) -> query::Builder<'_> {
        Transaction::table(self, &table_name(table, alias))
    }

    fn get_schema_builder(&mut self) -> schema::Builder<'_> {
        Transaction::get_schema_builder(self)
    }

    fn get_database_name(&self) -> &str {
        self.query_connection().get_database_name()
    }

    fn select<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        _use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Row>>> {
        Box::pin(Transaction::select(self, sql, bindings))
    }

    fn select_one<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        _use_read: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<Option<Row>>> {
        Box::pin(Transaction::select_one(self, sql, bindings))
    }

    fn cursor<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
        _use_read: bool,
    ) -> DatabaseCursor<'a> {
        Transaction::cursor(self, sql, bindings)
    }

    fn statement<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(Transaction::statement(self, sql, bindings))
    }

    fn affecting_statement<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(Transaction::affecting_statement(self, sql, bindings))
    }

    fn unprepared<'a>(&'a mut self, sql: &'a str) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(Transaction::unprepared(self, sql))
    }
}

fn table_name<'a>(table: &'a str, alias: Option<&str>) -> Cow<'a, str> {
    match alias {
        Some(alias) => Cow::Owned(format!("{table} as {alias}")),
        None => Cow::Borrowed(table),
    }
}

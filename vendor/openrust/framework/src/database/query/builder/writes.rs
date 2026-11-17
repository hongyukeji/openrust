use std::sync::Arc;

use crate::database::{DatabaseError, DatabaseResult, Transaction, Value};

use super::{Builder, Executor};
use crate::database::query::{Record, UpsertUpdate, grammars::Grammar};

mod json;

#[allow(non_snake_case, reason = "Matches Laravel's Query Builder API")]
impl Builder<'_> {
    pub async fn insert(mut self, rows: &[Record]) -> DatabaseResult<bool> {
        if rows.is_empty() {
            return Ok(true);
        }
        let (sql, bindings) = self.compile_insert(rows, false)?;
        match &mut self.executor {
            Executor::Connection(connection) => connection.insert(&sql, &bindings).await,
            Executor::Transaction(transaction) => transaction.insert(&sql, &bindings).await,
            Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery")),
        }
    }

    pub async fn insertOrIgnore(mut self, rows: &[Record]) -> DatabaseResult<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let (sql, bindings) = self.compile_insert(rows, true)?;
        self.affect(&sql, &bindings).await
    }

    pub async fn insertGetId(self, values: Record) -> DatabaseResult<Value> {
        self.insert_get_id_for(values, "id").await
    }

    pub(crate) async fn insert_get_id_for(
        mut self,
        values: Record,
        key: &str,
    ) -> DatabaseResult<Value> {
        let (mut sql, bindings) = self.compile_insert(&[values], false)?;
        if self.grammar.is_postgres() {
            sql.push_str(" returning ");
            sql.push_str(&self.grammar.column(key)?);
        } else if self.grammar.is_sql_server() {
            sql.push_str("; select cast(scope_identity() as bigint)");
        }
        match &mut self.executor {
            Executor::Connection(connection) => connection.insert_get_id(&sql, &bindings).await,
            Executor::Transaction(transaction) => transaction.insert_get_id(&sql, &bindings).await,
            Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery")),
        }
    }

    pub async fn insertUsing(self, columns: &[&str], source: &Builder<'_>) -> DatabaseResult<u64> {
        self.insert_using(columns, source, false).await
    }

    pub async fn insertOrIgnoreUsing(
        self,
        columns: &[&str],
        source: &Builder<'_>,
    ) -> DatabaseResult<u64> {
        self.insert_using(columns, source, true).await
    }

    async fn insert_using(
        mut self,
        columns: &[&str],
        source: &Builder<'_>,
        ignore: bool,
    ) -> DatabaseResult<u64> {
        let mut sql = self.insert_target(ignore)?;
        let source = self.subquery_expression(source)?;
        if !columns.is_empty() && columns != ["*"] {
            let columns = self.write_columns(columns.iter().copied())?;
            sql.push_str(&format!(" ({columns})"));
        }
        sql.push(' ');
        sql.push_str(&source.sql);
        self.affect(&sql, &source.bindings).await
    }

    pub async fn upsert(
        self,
        rows: &[Record],
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> DatabaseResult<u64> {
        let update = update.map(|columns| {
            columns
                .iter()
                .map(|column| UpsertUpdate::FromInsert(column))
                .collect::<Vec<_>>()
        });
        self.upsert_assignments(rows, unique_by, update.as_deref())
            .await
    }

    /// Inserts rows or applies the supplied assignments to rows that conflict.
    pub async fn upsertWith(
        self,
        rows: &[Record],
        unique_by: &[&str],
        update: &[UpsertUpdate<'_>],
    ) -> DatabaseResult<u64> {
        self.upsert_assignments(rows, unique_by, Some(update)).await
    }

    async fn upsert_assignments(
        mut self,
        rows: &[Record],
        unique_by: &[&str],
        update: Option<&[UpsertUpdate<'_>]>,
    ) -> DatabaseResult<u64> {
        if unique_by.is_empty() || unique_by.iter().any(|column| column.is_empty()) {
            return Err(DatabaseError::InvalidQuery("upsert conflict columns"));
        }
        let Some(first) = rows.first() else {
            return Ok(0);
        };
        if update.is_some_and(<[UpsertUpdate<'_>]>::is_empty) {
            return self.insert(rows).await.map(u64::from);
        }
        let default_update;
        let update = match update {
            Some(update) => update,
            None => {
                default_update = first
                    .keys()
                    .map(|column| UpsertUpdate::FromInsert(column))
                    .collect::<Vec<_>>();
                &default_update
            }
        };
        if update.is_empty() {
            return Err(DatabaseError::InvalidQuery("upsert update columns"));
        }
        if self.grammar.is_sql_server() {
            return self.sql_server_upsert(rows, unique_by, update).await;
        }
        let (mut sql, mut bindings) = self.compile_insert(rows, false)?;
        let mysql = self.grammar.is_mysql();
        let alias = mysql && self.grammar.uses_upsert_alias();
        if mysql {
            if alias {
                sql.push_str(" as laravel_upsert_alias");
            }
            sql.push_str(" on duplicate key update ");
        } else {
            let conflict = self.write_columns(unique_by.iter().copied())?;
            sql.push_str(&format!(" on conflict ({conflict}) do update set "));
        }
        let mut assignments = Vec::with_capacity(update.len());
        for assignment in update {
            let (column, value) = match assignment {
                UpsertUpdate::FromInsert(column) => {
                    if !first.contains_key(*column) {
                        return Err(DatabaseError::InvalidQuery("upsert update columns"));
                    }
                    let column = self.grammar.column(column)?;
                    let value = if !mysql {
                        format!("\"excluded\".{column}")
                    } else if alias {
                        format!("`laravel_upsert_alias`.{column}")
                    } else {
                        format!("values({column})")
                    };
                    (column, value)
                }
                UpsertUpdate::Value(column, value) => {
                    bindings.push(value.clone());
                    (self.grammar.column(column)?, "?".into())
                }
                UpsertUpdate::Expression(column, expression) => {
                    bindings.extend(expression.bindings.iter().cloned());
                    (self.grammar.column(column)?, expression.sql.clone())
                }
            };
            assignments.push(format!("{column} = {value}"));
        }
        sql.push_str(&assignments.join(", "));
        self.affect(&sql, &bindings).await
    }

    async fn sql_server_upsert(
        mut self,
        rows: &[Record],
        unique_by: &[&str],
        update: &[UpsertUpdate<'_>],
    ) -> DatabaseResult<u64> {
        if !self.joins.is_empty() {
            return Err(DatabaseError::UnsupportedOperation(
                "SQL Server joined upsert",
            ));
        }
        let first = &rows[0];
        let keys = first.keys().map(String::as_str).collect::<Vec<_>>();
        let columns = self.write_columns(keys.iter().copied())?;
        let (values, mut bindings) = value_parameters(rows)?;
        let table = self.grammar.table(self.source.table_name()?)?;
        let mut match_columns = Vec::with_capacity(unique_by.len());
        for key in unique_by {
            if !first.contains_key(*key) {
                return Err(DatabaseError::InvalidQuery("upsert conflict columns"));
            }
            let column = self.grammar.column(key)?;
            match_columns.push(format!("[target].{column} = [source].{column}"));
        }
        let mut assignments = Vec::with_capacity(update.len());
        for assignment in update {
            let (column, expression) = match assignment {
                UpsertUpdate::FromInsert(column) => {
                    if !first.contains_key(*column) {
                        return Err(DatabaseError::InvalidQuery("upsert update columns"));
                    }
                    (
                        *column,
                        format!("[source].{}", self.grammar.column(column)?),
                    )
                }
                UpsertUpdate::Value(column, value) => {
                    bindings.push(value.clone());
                    (*column, "?".into())
                }
                UpsertUpdate::Expression(column, expression) => {
                    bindings.extend(expression.bindings.iter().cloned());
                    (*column, expression.sql.clone())
                }
            };
            assignments.push(format!("{} = {expression}", self.grammar.column(column)?));
        }
        let inserted = keys
            .iter()
            .map(|column| {
                self.grammar
                    .column(column)
                    .map(|column| format!("[source].{column}"))
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        let sql = format!(
            "merge into {table} with (holdlock) as [target] using (values {values}) as [source] ({columns}) on {} when matched then update set {} when not matched then insert ({columns}) values ({});",
            match_columns.join(" and "),
            assignments.join(", "),
            inserted.join(", ")
        );
        self.affect(&sql, &bindings).await
    }

    pub async fn updateOrInsert(self, attributes: Record, values: Record) -> DatabaseResult<bool> {
        self.updateOrInsertWith(attributes, |_| values).await
    }

    pub async fn updateOrInsertWith(
        mut self,
        mut attributes: Record,
        values: impl FnOnce(bool) -> Record,
    ) -> DatabaseResult<bool> {
        for (column, value) in &attributes {
            self.predicates.compare(false, column, "=", value.clone());
        }
        let exists = self.exists_query().await?;
        let values = values(exists);
        if !exists {
            attributes.extend(values);
            return self.insert(&[attributes]).await;
        }
        if values.is_empty() {
            return Ok(true);
        }
        self.limit(1).update(values).await.map(|count| count > 0)
    }

    pub async fn update(self, values: Record) -> DatabaseResult<u64> {
        self.update_values(values, Record::new(), "+").await
    }

    pub async fn increment(
        self,
        column: &str,
        amount: impl Into<Value>,
        extra: Record,
    ) -> DatabaseResult<u64> {
        self.incrementEach(Record::from([(column.into(), amount.into())]), extra)
            .await
    }

    pub async fn decrement(
        self,
        column: &str,
        amount: impl Into<Value>,
        extra: Record,
    ) -> DatabaseResult<u64> {
        self.decrementEach(Record::from([(column.into(), amount.into())]), extra)
            .await
    }

    pub async fn incrementEach(self, columns: Record, extra: Record) -> DatabaseResult<u64> {
        self.update_values(extra, columns, "+").await
    }

    pub async fn decrementEach(self, columns: Record, extra: Record) -> DatabaseResult<u64> {
        self.update_values(extra, columns, "-").await
    }

    async fn update_values(
        mut self,
        mut values: Record,
        arithmetic: Record,
        operator: &str,
    ) -> DatabaseResult<u64> {
        self.validate_write_query()?;
        let table = self.source.table_name()?;
        let mut sql = format!("update {}", self.grammar.table(table)?);
        let mut bindings = Vec::new();
        if self.grammar.is_mysql() {
            self.compile_joins(&mut sql, &mut bindings)?;
        }
        let mut assignments = Vec::new();
        for (column, value) in arithmetic {
            if !matches!(
                &value,
                Value::Integer(_) | Value::Unsigned(_) | Value::Real(_)
            ) {
                return Err(DatabaseError::InvalidQuery("increment amount"));
            }
            values.remove(&column);
            let column = self.grammar.column(&column)?;
            assignments.push(format!("{column} = {column} {operator} ?"));
            bindings.push(value);
        }
        let (columns, parameters) = json::compile_assignments(&self.grammar, values)?;
        assignments.extend(columns);
        bindings.extend(parameters);
        if assignments.is_empty() {
            return Err(DatabaseError::InvalidQuery("update assignments"));
        }
        sql.push_str(" set ");
        sql.push_str(&assignments.join(", "));
        self.compile_write_tail(table, &mut sql, &mut bindings)?;
        self.affect(&sql, &bindings).await
    }

    pub async fn delete(mut self) -> DatabaseResult<u64> {
        self.validate_write_query()?;
        let table = self.source.table_name()?;
        let target = self.grammar.table(table)?;
        let mut bindings = Vec::new();
        let mut sql = if self.grammar.is_mysql() && !self.joins.is_empty() {
            let alias = self.grammar.table(table_reference(table))?;
            let mut sql = format!("delete {alias} from {target}");
            self.compile_joins(&mut sql, &mut bindings)?;
            sql
        } else {
            format!("delete from {target}")
        };
        self.compile_write_tail(table, &mut sql, &mut bindings)?;
        self.affect(&sql, &bindings).await
    }

    pub async fn truncate(mut self) -> DatabaseResult<()> {
        let table = self.source.table_name()?;
        let target = self.grammar.table(table)?;
        if table
            .as_bytes()
            .windows(4)
            .any(|part| part.eq_ignore_ascii_case(b" as "))
        {
            return Err(DatabaseError::InvalidQuery("truncate table alias"));
        }
        let native_sql = if self.grammar.is_postgres() {
            Some(self.grammar.postgres_truncate(&target))
        } else if self.grammar.is_mysql() || self.grammar.is_sql_server() {
            Some(format!("truncate table {target}"))
        } else {
            None
        };
        if let Some(sql) = native_sql {
            return match &mut self.executor {
                Executor::Connection(connection) => {
                    connection.statement(&sql, &[]).await.map(|_| ())
                }
                Executor::Transaction(_) if self.grammar.is_mysql() => Err(
                    DatabaseError::InvalidQuery("MySQL truncate implicitly commits"),
                ),
                Executor::Transaction(transaction) => {
                    transaction.statement(&sql, &[]).await.map(|_| ())
                }
                Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery")),
            };
        }

        let (schema, name) = table
            .rsplit_once('.')
            .map_or((None, table), |(schema, name)| {
                (Some(schema.to_owned()), name)
            });
        let query = Arc::new(SqliteTruncate {
            grammar: self.grammar.clone(),
            schema,
            name: format!("{}{name}", self.grammar.table_prefix()),
        });
        match &mut self.executor {
            Executor::Connection(connection) => {
                connection
                    .transaction(
                        |transaction| Box::pin(truncate_sqlite(transaction, query.clone())),
                        1,
                    )
                    .await
            }
            Executor::Transaction(transaction) => {
                transaction
                    .transaction(|transaction| Box::pin(truncate_sqlite(transaction, query)))
                    .await
            }
            Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery")),
        }
    }

    fn validate_write_query(&self) -> DatabaseResult<()> {
        if self.group_limit.is_some()
            || !self.unions.is_empty()
            || !self.groups.is_empty()
            || !self.havings.is_empty()
            || self.lock.is_some()
        {
            return Err(DatabaseError::InvalidQuery("write query clauses"));
        }
        if self.grammar.is_mysql()
            && (self.offset.is_some()
                || (!self.joins.is_empty() && (self.limit.is_some() || !self.orders.is_empty())))
        {
            return Err(DatabaseError::InvalidQuery("MySQL write pagination"));
        }
        if !self.grammar.is_mysql() && self.offset.is_some() && self.limit.is_none() {
            return Err(DatabaseError::InvalidQuery(
                "SQLite write offset requires limit",
            ));
        }
        Ok(())
    }

    fn compile_write_tail(
        &self,
        table: &str,
        sql: &mut String,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<()> {
        if !self.grammar.is_mysql() && (!self.joins.is_empty() || self.limit.is_some()) {
            let reference = table_reference(table);
            let reference = self.grammar.table(reference)?;
            let identity = if self.grammar.is_postgres() {
                "ctid"
            } else {
                "rowid"
            };
            sql.push_str(&format!(
                " where \"{identity}\" in (select {reference}.\"{identity}\" from {}",
                self.grammar.table(table)?
            ));
            self.compile_joins(sql, bindings)?;
            self.compile_predicates(sql, bindings)?;
            self.compile_orders(&self.orders, sql, bindings)?;
            Grammar::pagination(sql, self.limit, self.offset);
            sql.push(')');
        } else {
            self.compile_predicates(sql, bindings)?;
            if self.grammar.is_mysql() && self.joins.is_empty() {
                self.compile_orders(&self.orders, sql, bindings)?;
                Grammar::pagination(sql, self.limit, None);
            }
        }
        Ok(())
    }

    fn compile_insert(
        &self,
        rows: &[Record],
        ignore: bool,
    ) -> DatabaseResult<(String, Vec<Value>)> {
        let first = &rows[0];
        let sql = self.insert_target(ignore)?;
        if first.is_empty() && rows.len() == 1 {
            let default = if self.grammar.is_mysql() {
                " () values ()"
            } else {
                " default values"
            };
            return Ok((format!("{sql}{default}"), Vec::new()));
        }
        let (values, bindings) = value_parameters(rows)?;
        let columns = self.write_columns(first.keys().map(String::as_str))?;
        Ok((format!("{sql} ({columns}) values {values}"), bindings))
    }

    fn insert_target(&self, ignore: bool) -> DatabaseResult<String> {
        let verb = match (ignore, self.grammar.is_mysql()) {
            (false, _) => "insert into",
            (true, true) => "insert ignore into",
            (true, false) => "insert or ignore into",
        };
        Ok(format!(
            "{verb} {}",
            self.grammar.table(self.source.table_name()?)?
        ))
    }

    fn write_columns<'a>(
        &self,
        columns: impl IntoIterator<Item = &'a str>,
    ) -> DatabaseResult<String> {
        columns
            .into_iter()
            .map(|column| self.grammar.column(column))
            .collect::<DatabaseResult<Vec<_>>>()
            .map(|columns| columns.join(", "))
    }

    async fn affect(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<u64> {
        match &mut self.executor {
            Executor::Connection(connection) => connection.affectingStatement(sql, bindings).await,
            Executor::Transaction(transaction) => {
                transaction.affectingStatement(sql, bindings).await
            }
            Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery")),
        }
    }
}

fn value_parameters(rows: &[Record]) -> DatabaseResult<(String, Vec<Value>)> {
    let first = &rows[0];
    if first.is_empty() || rows.iter().any(|row| !row.keys().eq(first.keys())) {
        return Err(DatabaseError::InvalidQuery("insert row columns"));
    }
    let placeholders = format!("({})", vec!["?"; first.len()].join(", "));
    let values = vec![placeholders.as_str(); rows.len()].join(", ");
    let bindings = rows.iter().flat_map(|row| row.values().cloned()).collect();
    Ok((values, bindings))
}

struct SqliteTruncate {
    grammar: Grammar,
    schema: Option<String>,
    name: String,
}

async fn truncate_sqlite(
    transaction: &mut Transaction,
    query: Arc<SqliteTruncate>,
) -> DatabaseResult<()> {
    let resolved;
    let schema = if let Some(schema) = &query.schema {
        schema.as_str()
    } else {
        resolved = transaction
            .scalar(
                "select t.schema from pragma_table_list as t join pragma_database_list as d on d.name = t.schema                  where t.name = ? collate nocase order by case d.name when 'temp' then -1 else d.seq end limit 1",
                &[query.name.as_str().into()],
            )
            .await?;
        match &resolved {
            Value::Text(schema) => schema,
            _ => "main",
        }
    };
    let schema = query.grammar.quote(schema)?;
    let name = query.grammar.quote(&query.name)?;
    let exists = transaction
        .selectOne(
            &format!("select 1 from {schema}.sqlite_schema where name = 'sqlite_sequence'"),
            &[],
        )
        .await?
        .is_some();
    if exists {
        transaction
            .delete(
                &format!("delete from {schema}.sqlite_sequence where name = ? collate nocase"),
                &[query.name.as_str().into()],
            )
            .await?;
    }
    transaction
        .delete(&format!("delete from {schema}.{name}"), &[])
        .await?;
    Ok(())
}

fn table_reference(table: &str) -> &str {
    if let Some(index) = table
        .as_bytes()
        .windows(4)
        .position(|part| part.eq_ignore_ascii_case(b" as "))
    {
        return &table[index + 4..];
    }
    table.rsplit('.').next().expect("at least one segment")
}

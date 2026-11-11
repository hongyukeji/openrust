use std::sync::Arc;

use crate::database::{DatabaseError, DatabaseResult, Transaction, Value};

use super::{Builder, Executor};
use crate::database::query::{Record, grammars::Grammar};

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

    pub async fn insertGetId(mut self, values: Record) -> DatabaseResult<Value> {
        let (sql, bindings) = self.compile_insert(&[values], false)?;
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
        mut self,
        rows: &[Record],
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> DatabaseResult<u64> {
        if unique_by.is_empty() || unique_by.iter().any(|column| column.is_empty()) {
            return Err(DatabaseError::InvalidQuery("upsert conflict columns"));
        }
        let Some(first) = rows.first() else {
            return Ok(0);
        };
        if update == Some(&[]) {
            return self.insert(rows).await.map(u64::from);
        }
        let (mut sql, bindings) = self.compile_insert(rows, false)?;
        let update = update.map_or_else(
            || first.keys().map(String::as_str).collect(),
            |columns| columns.to_vec(),
        );
        if update.is_empty() || update.iter().any(|column| !first.contains_key(*column)) {
            return Err(DatabaseError::InvalidQuery("upsert update columns"));
        }
        if self.grammar.is_mysql() {
            if self.grammar.uses_upsert_alias() {
                sql.push_str(" as laravel_upsert_alias");
            }
            sql.push_str(" on duplicate key update ");
        } else {
            let conflict = self.write_columns(unique_by.iter().copied())?;
            if conflict.is_empty() {
                return Err(DatabaseError::InvalidQuery("upsert conflict columns"));
            }
            sql.push_str(&format!(" on conflict ({conflict}) do update set "));
        }
        let columns = update
            .iter()
            .map(|column| {
                let column = self.grammar.column(column)?;
                let value = if !self.grammar.is_mysql() {
                    format!("\"excluded\".{column}")
                } else if self.grammar.uses_upsert_alias() {
                    format!("`laravel_upsert_alias`.{column}")
                } else {
                    format!("values({column})")
                };
                Ok(format!("{column} = {value}"))
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        sql.push_str(&columns.join(", "));
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
        self.update_values(values, None).await
    }

    pub async fn increment(
        self,
        column: &str,
        amount: impl Into<Value>,
        extra: Record,
    ) -> DatabaseResult<u64> {
        self.update_values(extra, Some((column, "+", amount.into())))
            .await
    }

    pub async fn decrement(
        self,
        column: &str,
        amount: impl Into<Value>,
        extra: Record,
    ) -> DatabaseResult<u64> {
        self.update_values(extra, Some((column, "-", amount.into())))
            .await
    }

    async fn update_values(
        mut self,
        mut values: Record,
        arithmetic: Option<(&str, &str, Value)>,
    ) -> DatabaseResult<u64> {
        self.validate_write_query()?;
        let table = self.source.table_name()?;
        let mut sql = format!("update {}", self.grammar.table(table)?);
        let mut bindings = Vec::new();
        if self.grammar.is_mysql() {
            self.compile_joins(&mut sql, &mut bindings)?;
        }
        let mut assignments = Vec::new();
        if let Some((column, operator, value)) = arithmetic {
            if !matches!(
                &value,
                Value::Integer(_) | Value::Unsigned(_) | Value::Real(_)
            ) {
                return Err(DatabaseError::InvalidQuery("increment amount"));
            }
            values.remove(column);
            let column = self.grammar.column(column)?;
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
        if self.grammar.is_mysql() {
            return match &self.executor {
                Executor::Connection(connection) => connection
                    .statement(&format!("truncate table {target}"), &[])
                    .await
                    .map(|_| ()),
                Executor::Transaction(_) => Err(DatabaseError::InvalidQuery(
                    "MySQL truncate implicitly commits",
                )),
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
        if !self.unions.is_empty()
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
            sql.push_str(&format!(
                " where \"rowid\" in (select {reference}.\"rowid\" from {}",
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
        if first.is_empty() || rows.iter().any(|row| !row.keys().eq(first.keys())) {
            return Err(DatabaseError::InvalidQuery("insert row columns"));
        }
        let columns = self.write_columns(first.keys().map(String::as_str))?;
        let placeholders = format!("({})", vec!["?"; first.len()].join(", "));
        let sql = format!(
            "{sql} ({columns}) values {}",
            vec![placeholders.as_str(); rows.len()].join(", ")
        );
        let bindings = rows.iter().flat_map(|row| row.values().cloned()).collect();
        Ok((sql, bindings))
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

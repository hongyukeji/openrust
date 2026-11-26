use std::{collections::BTreeMap, sync::Arc};

use crate::database::{DatabaseError, DatabaseResult, Row, Transaction, Value};

use super::{Builder, Executor};
use crate::database::query::{Record, UpdateValue, UpsertUpdate, ValueOperand, grammars::Grammar};

mod json;

impl Builder<'_> {
    /// 将指定插入列中的字节参数按数据库的文本列规则写入。
    #[doc(hidden)]
    pub fn with_binary_text_column(mut self, column: &str) -> Self {
        self.binary_text_column = Some(column.into());
        self
    }

    #[track_caller]
    pub fn insert(mut self, rows: &[Record]) -> impl Future<Output = DatabaseResult<bool>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            if rows.is_empty() {
                return Ok(true);
            }
            self.apply_before_query_callbacks()?;
            let (sql, bindings) = self.compile_insert(rows, false)?;
            match &mut self.executor {
                Executor::Connection(connection) => connection.insert(&sql, &bindings).await,
                Executor::Transaction(transaction) => transaction.insert(&sql, &bindings).await,
                Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery")),
            }
        })
    }

    #[track_caller]
    pub fn insert_or_ignore(
        mut self,
        rows: &[Record],
    ) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            if rows.is_empty() {
                return Ok(0);
            }
            self.apply_before_query_callbacks()?;
            let (sql, bindings) = self.compile_insert(rows, true)?;
            self.affect(&sql, &bindings).await
        })
    }

    pub async fn insert_or_ignore_returning(
        mut self,
        rows: &[Record],
        returning: &[&str],
        unique_by: Option<&[&str]>,
    ) -> DatabaseResult<Vec<Row>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        if returning.is_empty() || unique_by.is_some_and(<[_]>::is_empty) {
            return Err(DatabaseError::InvalidQuery("insert returning columns"));
        }
        if self.grammar.is_mysql() || self.grammar.is_sql_server() {
            return Err(DatabaseError::UnsupportedOperation(
                "insert or ignore returning",
            ));
        }
        self.apply_before_query_callbacks()?;
        let (mut sql, bindings) = self.compile_insert(rows, false)?;
        sql.push_str(" on conflict");
        if let Some(columns) = unique_by {
            sql.push_str(" (");
            sql.push_str(&self.write_columns(columns.iter().copied())?);
            sql.push(')');
        }
        let projection = returning
            .iter()
            .map(|column| self.grammar.projection(column))
            .collect::<DatabaseResult<Vec<_>>>()?;
        sql.push_str(" do nothing returning ");
        sql.push_str(&projection.join(", "));
        match &mut self.executor {
            Executor::Connection(connection) => connection.insert_returning(&sql, &bindings).await,
            Executor::Transaction(transaction) => {
                transaction.insert_returning(&sql, &bindings).await
            }
            Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery")),
        }
    }

    #[track_caller]
    pub fn insert_get_id(self, values: Record) -> impl Future<Output = DatabaseResult<Value>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.insert_get_id_for(values, "id").await
        })
    }

    /// Uses a named PostgreSQL return column or the driver's native generated ID.
    pub async fn insert_get_id_for(mut self, values: Record, key: &str) -> DatabaseResult<Value> {
        self.apply_before_query_callbacks()?;
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

    pub async fn insert_using(
        self,
        columns: &[&str],
        source: &mut Builder<'_>,
    ) -> DatabaseResult<u64> {
        self.insert_using_with_ignore(columns, source, false).await
    }

    pub async fn insert_or_ignore_using(
        self,
        columns: &[&str],
        source: &mut Builder<'_>,
    ) -> DatabaseResult<u64> {
        self.insert_using_with_ignore(columns, source, true).await
    }

    async fn insert_using_with_ignore(
        mut self,
        columns: &[&str],
        source: &mut Builder<'_>,
        ignore: bool,
    ) -> DatabaseResult<u64> {
        self.apply_before_query_callbacks()?;
        let mut sql = self.insert_target(ignore)?;
        let source = self.subquery_expression(source)?;
        if !columns.is_empty() && columns != ["*"] {
            let columns = self.write_columns(columns.iter().copied())?;
            sql.push_str(&format!(" ({columns})"));
        }
        sql.push(' ');
        sql.push_str(&source.sql);
        if ignore && self.grammar.is_postgres() {
            sql.push_str(" on conflict do nothing");
        }
        self.affect(&sql, &source.bindings).await
    }

    #[track_caller]
    pub fn upsert(
        self,
        rows: &[Record],
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            let update = update.map(|columns| {
                columns
                    .iter()
                    .map(|column| UpsertUpdate::FromInsert(column))
                    .collect::<Vec<_>>()
            });
            self.upsert_assignments(rows, unique_by, update.as_deref())
                .await
        })
    }

    /// Inserts rows or applies the supplied assignments to rows that conflict.
    pub async fn upsert_with(
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
        self.apply_before_query_callbacks()?;
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
        let (values, mut bindings) = value_parameters(rows, None)?;
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

    pub async fn update_or_insert(
        self,
        attributes: Record,
        values: Record,
    ) -> DatabaseResult<bool> {
        self.update_or_insert_with(attributes, |_| values).await
    }

    pub async fn update_or_insert_with(
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

    #[track_caller]
    pub fn update(self, values: Record) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.update_values(values, Record::new(), "+").await
        })
    }

    /// Updates column values using bound data, expressions, or scalar subqueries.
    pub async fn update_with(
        self,
        values: BTreeMap<String, UpdateValue<'_, '_>>,
    ) -> DatabaseResult<u64> {
        let values = values
            .into_iter()
            .map(|(column, value)| {
                let value = match value {
                    UpdateValue::Operand(value) => value,
                    UpdateValue::Subquery(query) => {
                        let mut expression = self.subquery_expression(query)?;
                        expression.sql = format!("({})", expression.sql);
                        ValueOperand::Expression(expression)
                    }
                };
                Ok((column, value))
            })
            .collect::<DatabaseResult<BTreeMap<_, _>>>()?;
        self.update_values(values, Record::new(), "+").await
    }

    /// Updates a PostgreSQL target using joined sources in the FROM clause.
    pub async fn update_from<V: Into<ValueOperand>>(
        mut self,
        values: BTreeMap<String, V>,
    ) -> DatabaseResult<u64> {
        if !self.grammar.is_postgres() {
            return Err(DatabaseError::UnsupportedOperation("updateFrom"));
        }
        self.apply_before_query_callbacks()?;
        let target = self.source.table_name()?;
        let table = self.grammar.table(target)?;
        let qualifier = self.grammar.table(table_reference(target))?;
        let (assignments, mut bindings) =
            json::compile_assignments(&self.grammar, values, Some(&qualifier))?;
        if assignments.is_empty() {
            return Err(DatabaseError::InvalidQuery("update assignments"));
        }
        let mut sql = format!("update {table} set {}", assignments.join(", "));
        let assignments_end = bindings.len();
        let mut source_lengths = Vec::new();
        for (index, join) in self.joins.iter().enumerate() {
            sql.push_str(if index == 0 { " from " } else { ", " });
            let start = bindings.len();
            sql.push_str(&join.source.compile(&self.grammar, None, &mut bindings)?);
            source_lengths.push(bindings.len() - start);
        }
        let sources_end = bindings.len();
        self.compile_predicates(&mut sql, &mut bindings)?;
        let where_end = bindings.len();
        let mut clause_lengths = Vec::new();
        let mut has_predicates = !self.predicates.is_empty();
        for join in &self.joins {
            if join.clause.0.is_empty() {
                clause_lengths.push(0);
                continue;
            }
            let start = bindings.len();
            if !has_predicates {
                sql.push_str(" where ");
            }
            join.clause.0.compile_continued(
                &self.grammar,
                &mut sql,
                &mut bindings,
                has_predicates,
            )?;
            clause_lengths.push(bindings.len() - start);
            has_predicates = true;
        }
        if self
            .binding_changes
            .as_ref()
            .is_some_and(|changes| changes[2].is_some())
        {
            // UPDATE FROM把来源放在WHERE前、连接条件放在WHERE后，按SQL位置分配join类别。
            let mut changed = self.natural_bindings(2)?;
            self.change_bindings(2, &mut changed, 0);
            let mut changed = changed.into_iter();
            let mut sources = Vec::new();
            let mut clauses = Vec::new();
            for (source, clause) in source_lengths.into_iter().zip(clause_lengths) {
                sources.extend(changed.by_ref().take(source));
                clauses.extend(changed.by_ref().take(clause));
            }
            clauses.extend(changed);
            let wheres = bindings[sources_end..where_end].to_vec();
            bindings.truncate(assignments_end);
            bindings.extend(sources);
            bindings.extend(wheres);
            bindings.extend(clauses);
        }
        self.affect(&sql, &bindings).await
    }

    #[track_caller]
    pub fn increment(
        self,
        column: &str,
        amount: impl Into<Value>,
        extra: Record,
    ) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.increment_each(Record::from([(column.into(), amount.into())]), extra)
                .await
        })
    }

    #[track_caller]
    pub fn decrement(
        self,
        column: &str,
        amount: impl Into<Value>,
        extra: Record,
    ) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.decrement_each(Record::from([(column.into(), amount.into())]), extra)
                .await
        })
    }

    pub async fn increment_each(self, columns: Record, extra: Record) -> DatabaseResult<u64> {
        self.update_values(extra, columns, "+").await
    }

    pub async fn decrement_each(self, columns: Record, extra: Record) -> DatabaseResult<u64> {
        self.update_values(extra, columns, "-").await
    }

    async fn update_values<V: Into<ValueOperand>>(
        mut self,
        mut values: BTreeMap<String, V>,
        arithmetic: Record,
        operator: &str,
    ) -> DatabaseResult<u64> {
        self.apply_before_query_callbacks()?;
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
            let target = if self.grammar.is_sql_server() && column.contains('.') {
                Some(json::write_column(&self.grammar, &column)?)
            } else {
                None
            };
            let source = self.grammar.column(&column)?;
            let target = target.as_deref().unwrap_or(&source);
            assignments.push(format!("{target} = {source} {operator} ?"));
            bindings.push(value);
        }
        let (columns, parameters) = json::compile_assignments(&self.grammar, values, None)?;
        assignments.extend(columns);
        bindings.extend(parameters);
        if assignments.is_empty() {
            return Err(DatabaseError::InvalidQuery("update assignments"));
        }
        let assignments = assignments.join(", ");
        if self.grammar.is_sql_server() {
            let sql = self.compile_sql_server_write(table, Some(&assignments), &mut bindings)?;
            return self.affect(&sql, &bindings).await;
        }
        sql.push_str(" set ");
        sql.push_str(&assignments);
        self.compile_write_tail(table, &mut sql, &mut bindings)?;
        self.affect(&sql, &bindings).await
    }

    pub(in crate::database) async fn delete_pruning_batch(
        &self,
        chunk: u64,
    ) -> DatabaseResult<u64> {
        let Executor::Connection(connection) = &self.executor else {
            return Err(DatabaseError::InvalidQuery("批量清理需要独立的数据库连接"));
        };
        let mut query = self.snapshot();
        query.executor = Executor::Connection(connection.clone());
        if query.limit.is_none_or(|limit| limit == 0) {
            query = query.limit(chunk);
        }
        query.apply_before_query_callbacks()?;
        if query.limit.is_none_or(|limit| limit == 0) {
            return Err(DatabaseError::InvalidQuery("批量清理须设置正数删除上限"));
        }
        if query.grammar.is_sql_server()
            && (!query.joins.is_empty() || query.offset.unwrap_or_default() > 0)
        {
            return Err(DatabaseError::UnsupportedOperation(
                "SQL Server 批量清理的连接查询或偏移",
            ));
        }
        query.delete().await
    }

    #[track_caller]
    pub fn delete(mut self) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.apply_before_query_callbacks()?;
            self.validate_write_query()?;
            let table = self.source.table_name()?;
            let mut bindings = Vec::new();
            if self.grammar.is_sql_server() {
                let sql = self.compile_sql_server_write(table, None, &mut bindings)?;
                return self.affect(&sql, &bindings).await;
            }
            let target = self.grammar.table(table)?;
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
        })
    }

    #[track_caller]
    pub fn truncate(mut self) -> impl Future<Output = DatabaseResult<()>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.apply_before_query_callbacks()?;
            let table = self.source.table_name()?;
            let target = self.grammar.table(table)?;
            if table_alias(table).is_some() {
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
        })
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
        if !self.grammar.is_mysql()
            && !self.grammar.is_sql_server()
            && self.offset.is_some()
            && self.limit.is_none()
        {
            return Err(DatabaseError::InvalidQuery(
                "SQLite write offset requires limit",
            ));
        }
        Ok(())
    }

    fn compile_sql_server_write(
        &self,
        table: &str,
        assignments: Option<&str>,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<String> {
        let table_sql = self.grammar.table(table)?;
        let top = self.limit.filter(|&limit| {
            limit > 0 && self.offset.unwrap_or_default() == 0 && self.joins.is_empty()
        });
        let ordered_top = top.filter(|_| !self.orders.is_empty());
        let reference = if table_alias(table).is_some() || ordered_top.is_some() {
            Some(self.grammar.table(table_reference(table))?)
        } else {
            None
        };
        let from_target = reference.is_some() || !self.joins.is_empty();
        let target = reference.as_deref().unwrap_or(&table_sql);
        let mut sql = match assignments {
            Some(_) => String::from("update "),
            None => String::from("delete "),
        };
        if let Some(limit) = top.filter(|_| ordered_top.is_none()) {
            sql.push_str(&format!("top ({limit}) "));
        }
        if !from_target && assignments.is_none() {
            sql.push_str("from ");
        }
        sql.push_str(target);
        if let Some(assignments) = assignments {
            sql.push_str(" set ");
            sql.push_str(assignments);
        }
        if let Some(limit) = ordered_top {
            sql.push_str(&format!(" from (select top ({limit}) * from {table_sql}"));
            self.compile_predicates(&mut sql, bindings)?;
            self.compile_bound_orders(&mut sql, bindings)?;
            sql.push_str(") as ");
            sql.push_str(target);
        } else {
            if from_target {
                sql.push_str(" from ");
                sql.push_str(&table_sql);
                self.compile_joins(&mut sql, bindings)?;
            }
            self.compile_predicates(&mut sql, bindings)?;
        }
        Ok(sql)
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
            let qualified_table = self.grammar.table(table)?;
            if self.grammar.is_postgres() {
                // 同一个 CTID 在不同分区中可能出现，写入必须同时匹配物理表。
                sql.push_str(&format!(
                    " where (\"tableoid\", \"ctid\") in (select {reference}.\"tableoid\", {reference}.\"ctid\" from {qualified_table}"
                ));
            } else {
                sql.push_str(&format!(
                    " where \"rowid\" in (select {reference}.\"rowid\" from {qualified_table}"
                ));
            }
            self.compile_table_hints(sql, None)?;
            self.compile_joins(sql, bindings)?;
            self.compile_predicates(sql, bindings)?;
            self.compile_bound_orders(sql, bindings)?;
            Grammar::pagination(sql, self.limit, self.offset);
            sql.push(')');
        } else {
            self.compile_predicates(sql, bindings)?;
            if self.grammar.is_mysql() && self.joins.is_empty() {
                self.compile_bound_orders(sql, bindings)?;
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
        let conflict = if ignore && self.grammar.is_postgres() {
            " on conflict do nothing"
        } else {
            ""
        };
        if first.is_empty() && rows.len() == 1 {
            let default = if self.grammar.is_mysql() {
                " () values ()"
            } else {
                " default values"
            };
            return Ok((format!("{sql}{default}{conflict}"), Vec::new()));
        }
        let binary_text = self.binary_text_column.as_deref().and_then(|column| {
            let expression = match self.get_connection().get_driver_name() {
                "sqlite" => "cast(? as text)",
                "pgsql" => "convert_from(?, 'UTF8')",
                _ => return None,
            };
            Some((column, expression))
        });
        let (values, bindings) = value_parameters(rows, binary_text)?;
        let columns = self.write_columns(first.keys().map(String::as_str))?;
        Ok((
            format!("{sql} ({columns}) values {values}{conflict}"),
            bindings,
        ))
    }

    fn insert_target(&self, ignore: bool) -> DatabaseResult<String> {
        let verb = if !ignore || self.grammar.is_postgres() {
            "insert into"
        } else if self.grammar.is_mysql() {
            "insert ignore into"
        } else if self.grammar.is_sql_server() {
            return Err(DatabaseError::UnsupportedOperation("insert or ignore"));
        } else {
            "insert or ignore into"
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
            Executor::Connection(connection) => connection.affecting_statement(sql, bindings).await,
            Executor::Transaction(transaction) => {
                transaction.affecting_statement(sql, bindings).await
            }
            Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery")),
        }
    }
}

fn value_parameters(
    rows: &[Record],
    binary_text: Option<(&str, &str)>,
) -> DatabaseResult<(String, Vec<Value>)> {
    let first = &rows[0];
    if first.is_empty() || rows.iter().any(|row| !row.keys().eq(first.keys())) {
        return Err(DatabaseError::InvalidQuery("insert row columns"));
    }
    let values = if let Some((column, expression)) = binary_text.filter(|(column, _)| {
        rows.iter()
            .any(|row| matches!(row.get(*column), Some(Value::Bytes(_))))
    }) {
        rows.iter()
            .map(|row| {
                let placeholders = row.iter().map(|(name, value)| {
                    if name == column && matches!(value, Value::Bytes(_)) {
                        expression
                    } else {
                        "?"
                    }
                });
                format!("({})", placeholders.collect::<Vec<_>>().join(", "))
            })
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        let placeholders = format!("({})", vec!["?"; first.len()].join(", "));
        vec![placeholders.as_str(); rows.len()].join(", ")
    };
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
        .select_one(
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
    table_alias(table).unwrap_or_else(|| table.rsplit('.').next().expect("at least one segment"))
}

fn table_alias(table: &str) -> Option<&str> {
    table
        .as_bytes()
        .windows(4)
        .position(|part| part.eq_ignore_ascii_case(b" as "))
        .map(|index| &table[index + 4..])
}

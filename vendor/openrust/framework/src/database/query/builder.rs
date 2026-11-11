use crate::database::{Connection, DatabaseError, DatabaseResult, Row, Transaction, Value};

use super::{
    Conditions, Expression,
    conditions::{date_methods, predicate_methods},
    grammars::{DatePart, Grammar},
};

enum Executor<'connection> {
    Connection(Connection),
    Transaction(&'connection mut Transaction),
}

/// A selected column name or an explicitly trusted expression.
#[derive(Clone)]
pub enum Column {
    Identifier(String),
    Raw(Expression),
}

impl From<&str> for Column {
    fn from(value: &str) -> Self {
        Self::Identifier(value.to_owned())
    }
}

impl From<Expression> for Column {
    fn from(value: Expression) -> Self {
        Self::Raw(value)
    }
}

enum Order {
    Column(String, String),
    Raw(Expression),
}

/// Builds and executes a query using the grammar of the selected connection.
pub struct Builder<'connection> {
    executor: Executor<'connection>,
    grammar: Grammar,
    table: String,
    columns: Vec<Column>,
    distinct: bool,
    predicates: Conditions,
    groups: Vec<Column>,
    havings: Conditions,
    orders: Vec<Order>,
    limit: Option<u64>,
    offset: Option<u64>,
    use_write: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's Query Builder API")]
impl<'connection> Builder<'connection> {
    pub(crate) fn connection(
        connection: Connection,
        grammar: Grammar,
        table: &str,
    ) -> Builder<'static> {
        Builder::new(Executor::Connection(connection), grammar, table)
    }

    pub(crate) fn transaction(
        transaction: &'connection mut Transaction,
        grammar: Grammar,
        table: &str,
    ) -> Self {
        Self::new(Executor::Transaction(transaction), grammar, table)
    }

    fn new(executor: Executor<'connection>, grammar: Grammar, table: &str) -> Self {
        Self {
            executor,
            grammar,
            table: table.into(),
            columns: Vec::new(),
            distinct: false,
            predicates: Conditions::default(),
            groups: Vec::new(),
            havings: Conditions::default(),
            orders: Vec::new(),
            limit: None,
            offset: None,
            use_write: false,
        }
    }

    fn conditions(&mut self) -> &mut Conditions {
        &mut self.predicates
    }

    predicate_methods!();

    pub fn from(mut self, table: &str) -> Self {
        self.table = table.into();
        self
    }

    pub fn select<C: Into<Column>>(mut self, columns: impl IntoIterator<Item = C>) -> Self {
        self.columns.clear();
        self.addSelect(columns)
    }

    pub fn addSelect<C: Into<Column>>(mut self, columns: impl IntoIterator<Item = C>) -> Self {
        self.columns.extend(columns.into_iter().map(Into::into));
        self
    }

    pub fn selectRaw(self, sql: &str, bindings: Vec<Value>) -> Self {
        self.addSelect([Expression::new(sql).with_bindings(bindings)])
    }

    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    pub fn groupBy(mut self, columns: &[&str]) -> Self {
        self.groups.extend(
            columns
                .iter()
                .map(|column| Column::Identifier((*column).into())),
        );
        self
    }

    pub fn groupByRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.groups
            .push(Column::Raw(Expression::new(sql).with_bindings(bindings)));
        self
    }

    pub fn having(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        self.havings.compare(false, column, operator, value.into());
        self
    }

    pub fn orHaving(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        self.havings.compare(true, column, operator, value.into());
        self
    }

    pub fn havingBetween(mut self, column: &str, bounds: [Value; 2]) -> Self {
        self.havings.between(false, column, bounds, false);
        self
    }

    pub fn havingRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.havings
            .raw(false, Expression::new(sql).with_bindings(bindings));
        self
    }

    pub fn orHavingRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.havings
            .raw(true, Expression::new(sql).with_bindings(bindings));
        self
    }

    pub fn orderBy(mut self, column: &str, direction: &str) -> Self {
        self.orders
            .push(Order::Column(column.into(), direction.into()));
        self
    }

    pub fn orderByDesc(self, column: &str) -> Self {
        self.orderBy(column, "desc")
    }

    pub fn orderByRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.orders
            .push(Order::Raw(Expression::new(sql).with_bindings(bindings)));
        self
    }

    pub fn reorder(mut self) -> Self {
        self.orders.clear();
        self
    }

    pub fn latest(self, column: &str) -> Self {
        self.orderByDesc(column)
    }

    pub fn oldest(self, column: &str) -> Self {
        self.orderBy(column, "asc")
    }

    pub fn limit(mut self, rows: u64) -> Self {
        self.limit = Some(rows);
        self
    }

    pub fn take(self, rows: u64) -> Self {
        self.limit(rows)
    }

    pub fn offset(mut self, rows: u64) -> Self {
        self.offset = Some(rows);
        self
    }

    pub fn skip(self, rows: u64) -> Self {
        self.offset(rows)
    }

    pub fn forPage(self, page: u64, per_page: u64) -> Self {
        self.offset(page.saturating_sub(1).saturating_mul(per_page))
            .limit(per_page)
    }

    pub fn useWritePdo(mut self) -> Self {
        self.use_write = true;
        self
    }

    pub fn toSql(&self) -> DatabaseResult<String> {
        self.compile(None).map(|(sql, _)| sql)
    }

    pub fn getBindings(&self) -> DatabaseResult<Vec<Value>> {
        self.compile(None).map(|(_, bindings)| bindings)
    }

    pub async fn get(mut self) -> DatabaseResult<Vec<Row>> {
        let (sql, bindings) = self.compile(None)?;
        self.execute(&sql, &bindings).await
    }

    pub async fn first(self) -> DatabaseResult<Option<Row>> {
        Ok(self.limit(1).get().await?.into_iter().next())
    }

    pub async fn find(self, id: impl Into<Value>) -> DatabaseResult<Option<Row>> {
        self.r#where("id", "=", id).first().await
    }

    pub async fn value(self, column: &str) -> DatabaseResult<Value> {
        self.select([column])
            .first()
            .await?
            .map_or(Ok(Value::Null), Row::scalar)
    }

    pub async fn pluck(self, column: &str) -> DatabaseResult<Vec<Value>> {
        self.select([column])
            .get()
            .await?
            .into_iter()
            .map(Row::scalar)
            .collect()
    }

    pub async fn exists(mut self) -> DatabaseResult<bool> {
        let (sql, bindings) = self.compile(None)?;
        let alias = self.grammar.column("exists")?;
        let sql = format!("select exists({sql}) as {alias}");
        let rows = self.execute(&sql, &bindings).await?;
        match first_value(rows)? {
            Value::Integer(value) => Ok(value != 0),
            Value::Unsigned(value) => Ok(value != 0),
            Value::Bool(value) => Ok(value),
            _ => Err(DatabaseError::InvalidQuery("exists result")),
        }
    }

    pub async fn doesntExist(self) -> DatabaseResult<bool> {
        self.exists().await.map(|value| !value)
    }

    pub async fn count(self, column: &str) -> DatabaseResult<u64> {
        match self.aggregate("count", column).await? {
            Value::Null => Ok(0),
            Value::Unsigned(count) => Ok(count),
            Value::Integer(count) => {
                u64::try_from(count).map_err(|_| DatabaseError::InvalidQuery("count result"))
            }
            _ => Err(DatabaseError::InvalidQuery("count result")),
        }
    }

    pub async fn min(self, column: &str) -> DatabaseResult<Value> {
        self.aggregate("min", column).await
    }

    pub async fn max(self, column: &str) -> DatabaseResult<Value> {
        self.aggregate("max", column).await
    }

    pub async fn sum(self, column: &str) -> DatabaseResult<Value> {
        match self.aggregate("sum", column).await? {
            Value::Null => Ok(0.into()),
            value => Ok(value),
        }
    }

    pub async fn avg(self, column: &str) -> DatabaseResult<Value> {
        self.aggregate("avg", column).await
    }

    pub async fn average(self, column: &str) -> DatabaseResult<Value> {
        self.avg(column).await
    }

    async fn aggregate(mut self, function: &'static str, column: &str) -> DatabaseResult<Value> {
        let (sql, bindings) = self.compile(Some((function, column)))?;
        first_value(self.execute(&sql, &bindings).await?)
    }

    async fn execute(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<Vec<Row>> {
        match &mut self.executor {
            Executor::Connection(connection) if self.use_write => {
                connection.selectFromWriteConnection(sql, bindings).await
            }
            Executor::Connection(connection) => connection.select(sql, bindings).await,
            Executor::Transaction(transaction) => transaction.select(sql, bindings).await,
        }
    }

    fn compile(&self, aggregate: Option<(&str, &str)>) -> DatabaseResult<(String, Vec<Value>)> {
        let mut sql = String::from("select ");
        let mut bindings = Vec::new();

        if let Some((function, column)) = aggregate {
            let distinct = if self.distinct && column != "*" {
                "distinct "
            } else {
                ""
            };
            let column = self.grammar.column(column)?;
            let alias = self.grammar.column("aggregate")?;
            sql.push_str(&format!("{function}({distinct}{column}) as {alias}"));
        } else {
            if self.distinct {
                sql.push_str("distinct ");
            }
            if self.columns.is_empty() {
                sql.push('*');
            } else {
                self.compile_columns(&self.columns, &mut sql, &mut bindings, true)?;
            }
        }

        sql.push_str(" from ");
        sql.push_str(&self.grammar.table(&self.table)?);

        if !self.predicates.is_empty() {
            sql.push_str(" where ");
            self.predicates
                .compile(&self.grammar, &mut sql, &mut bindings)?;
        }

        if !self.groups.is_empty() {
            sql.push_str(" group by ");
            self.compile_columns(&self.groups, &mut sql, &mut bindings, false)?;
        }

        if !self.havings.is_empty() {
            sql.push_str(" having ");
            self.havings
                .compile(&self.grammar, &mut sql, &mut bindings)?;
        }

        if !self.orders.is_empty() && (aggregate.is_none() || !self.groups.is_empty()) {
            self.compile_orders(&mut sql, &mut bindings)?;
        }

        Grammar::pagination(&mut sql, self.limit, self.offset);
        Ok((sql, bindings))
    }

    fn compile_columns(
        &self,
        columns: &[Column],
        sql: &mut String,
        bindings: &mut Vec<Value>,
        projection: bool,
    ) -> DatabaseResult<()> {
        for (index, column) in columns.iter().enumerate() {
            if index != 0 {
                sql.push_str(", ");
            }
            match column {
                Column::Identifier(column) => {
                    let name = if projection {
                        self.grammar.projection(column)?
                    } else {
                        self.grammar.column(column)?
                    };
                    sql.push_str(&name);
                }
                Column::Raw(expression) => {
                    sql.push_str(&expression.sql);
                    bindings.extend_from_slice(&expression.bindings);
                }
            }
        }
        Ok(())
    }

    fn compile_orders(&self, sql: &mut String, bindings: &mut Vec<Value>) -> DatabaseResult<()> {
        sql.push_str(" order by ");
        for (index, order) in self.orders.iter().enumerate() {
            if index != 0 {
                sql.push_str(", ");
            }
            match order {
                Order::Column(column, direction) => {
                    let direction = direction.to_ascii_lowercase();
                    if direction != "asc" && direction != "desc" {
                        return Err(DatabaseError::InvalidQuery("order direction"));
                    }
                    sql.push_str(&self.grammar.column(column)?);
                    sql.push(' ');
                    sql.push_str(&direction);
                }
                Order::Raw(expression) => {
                    sql.push_str(&expression.sql);
                    bindings.extend_from_slice(&expression.bindings);
                }
            }
        }
        Ok(())
    }
}

fn first_value(rows: Vec<Row>) -> DatabaseResult<Value> {
    rows.into_iter().next().map_or(Ok(Value::Null), Row::scalar)
}

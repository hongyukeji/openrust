use crate::database::{DatabaseError, DatabaseResult, Value};

use super::{Builder, Executor, Order};
use crate::database::query::{Expression, JoinClause, grammars::Grammar};

pub(super) enum Source {
    Table(String),
    Query(Expression, String),
}

impl Source {
    pub(super) fn compile(
        &self,
        grammar: &Grammar,
        database: Option<&str>,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<String> {
        match self {
            Self::Table(table) => match database {
                Some(database) => grammar.table_in_database(table, database),
                None => grammar.table(table),
            },
            Self::Query(query, alias) => {
                bindings.extend_from_slice(&query.bindings);
                Ok(format!("({}) as {}", query.sql, grammar.table(alias)?))
            }
        }
    }

    pub(super) fn table_name(&self) -> DatabaseResult<&str> {
        match self {
            Self::Table(table) => Ok(table),
            Self::Query(_, _) => Err(DatabaseError::InvalidQuery("write target")),
        }
    }
}

#[derive(Clone, Copy)]
enum JoinType {
    Inner,
    Left,
    Right,
    Cross,
    Lateral {
        operator: &'static str,
        condition: &'static str,
    },
}

pub(super) struct Join {
    source: Source,
    kind: JoinType,
    clause: JoinClause,
}

impl Join {
    pub(super) fn compile(
        &self,
        grammar: &Grammar,
        sql: &mut String,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<()> {
        sql.push_str(match self.kind {
            JoinType::Inner => " inner join ",
            JoinType::Left => " left join ",
            JoinType::Right => " right join ",
            JoinType::Cross => " cross join ",
            JoinType::Lateral { operator, .. } => operator,
        });
        sql.push_str(&self.source.compile(grammar, None, bindings)?);
        if let JoinType::Lateral { condition, .. } = self.kind {
            sql.push_str(condition);
        } else if !self.clause.0.is_empty() {
            sql.push_str(" on ");
            self.clause.0.compile(grammar, sql, bindings)?;
        }
        Ok(())
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Query Builder API")]
impl Builder<'_> {
    /// Starts a query to be embedded as SQL in another builder on this connection.
    pub fn subquery(&self, table: &str) -> Builder<'static> {
        Builder::new(
            Executor::Detached(self.executor.source().clone()),
            self.grammar.clone(),
            table,
        )
    }

    pub(in crate::database) fn relation_qualifier(&self) -> &str {
        match &self.source {
            Source::Table(table) => table.rsplit_once(" as ").map_or(table, |(_, alias)| alias),
            Source::Query(_, alias) => alias,
        }
    }

    pub(super) fn subquery_expression(&self, query: &Builder<'_>) -> DatabaseResult<Expression> {
        let outer = self.executor.source();
        let inner = query.executor.source();
        let qualification = if outer.same_query_source(inner) {
            None
        } else {
            if !self.grammar.is_mysql() || !query.grammar.is_mysql() {
                return Err(DatabaseError::InvalidQuery("subquery connection"));
            }
            let Some((outer_database, inner_database)) =
                outer.pools.mysql_schemas_for_subquery(&inner.pools)?
            else {
                return Err(DatabaseError::InvalidQuery("subquery connection"));
            };
            (outer_database != inner_database).then_some(inner_database)
        };
        let (sql, bindings) = query.compile_with_database(None, qualification.as_deref())?;
        Ok(Expression::new(sql).with_bindings(bindings))
    }

    pub fn selectSub(self, query: &Builder<'_>, alias: &str) -> DatabaseResult<Self> {
        let query = self.subquery_expression(query)?;
        let alias = self.grammar.column(alias)?;
        Ok(self.selectRaw(&format!("({}) as {alias}", query.sql), query.bindings))
    }

    pub(in crate::database) fn select_exists(
        self,
        query: &Builder<'_>,
        alias: &str,
    ) -> DatabaseResult<Self> {
        let query = self.subquery_expression(query)?;
        let alias = self.grammar.quote(alias)?;
        Ok(self.selectRaw(
            &format!("exists ({}) as {alias}", query.sql),
            query.bindings,
        ))
    }

    pub(in crate::database) fn select_related_aggregate(
        self,
        function: &'static str,
        column: &str,
    ) -> DatabaseResult<Self> {
        let column = if column == "*" {
            column.to_owned()
        } else if column.contains('.') {
            self.grammar.column(column)?
        } else {
            let table = self.source.table_name()?;
            let alias = table
                .as_bytes()
                .windows(4)
                .position(|part| part.eq_ignore_ascii_case(b" as "));
            let qualifier = alias.map_or(table, |index| &table[index + 4..]);
            self.grammar.column(&format!("{qualifier}.{column}"))?
        };
        Ok(self
            .reorder()
            .select([Expression::new(format!("{function}({column})"))]))
    }

    pub(in crate::database) fn select_default(mut self, column: &str) -> Self {
        if self.columns.is_empty() {
            self.columns.push(column.into());
        }
        self
    }

    pub fn fromSub(mut self, query: &Builder<'_>, alias: &str) -> DatabaseResult<Self> {
        self.source = Source::Query(self.subquery_expression(query)?, alias.into());
        Ok(self)
    }

    pub fn join(self, table: &str, column: &str, operator: &str, other: &str) -> Self {
        self.joinWith(table, |join| join.on(column, operator, other))
    }

    pub fn joinWith(self, table: &str, configure: impl FnOnce(JoinClause) -> JoinClause) -> Self {
        self.add_join(
            Source::Table(table.into()),
            JoinType::Inner,
            configure(JoinClause::default()),
        )
    }

    pub fn leftJoin(self, table: &str, column: &str, operator: &str, other: &str) -> Self {
        self.leftJoinWith(table, |join| join.on(column, operator, other))
    }

    pub fn leftJoinWith(
        self,
        table: &str,
        configure: impl FnOnce(JoinClause) -> JoinClause,
    ) -> Self {
        self.add_join(
            Source::Table(table.into()),
            JoinType::Left,
            configure(JoinClause::default()),
        )
    }

    pub fn rightJoin(self, table: &str, column: &str, operator: &str, other: &str) -> Self {
        self.add_join(
            Source::Table(table.into()),
            JoinType::Right,
            JoinClause::default().on(column, operator, other),
        )
    }

    pub fn crossJoin(self, table: &str) -> Self {
        self.add_join(
            Source::Table(table.into()),
            JoinType::Cross,
            JoinClause::default(),
        )
    }

    pub fn joinSub(
        self,
        query: &Builder<'_>,
        alias: &str,
        configure: impl FnOnce(JoinClause) -> JoinClause,
    ) -> DatabaseResult<Self> {
        self.join_sub(query, alias, JoinType::Inner, configure)
    }

    pub fn leftJoinSub(
        self,
        query: &Builder<'_>,
        alias: &str,
        configure: impl FnOnce(JoinClause) -> JoinClause,
    ) -> DatabaseResult<Self> {
        self.join_sub(query, alias, JoinType::Left, configure)
    }

    pub fn rightJoinSub(
        self,
        query: &Builder<'_>,
        alias: &str,
        configure: impl FnOnce(JoinClause) -> JoinClause,
    ) -> DatabaseResult<Self> {
        self.join_sub(query, alias, JoinType::Right, configure)
    }

    pub fn joinLateral(self, query: &Builder<'_>, alias: &str) -> DatabaseResult<Self> {
        self.join_lateral(query, alias, false)
    }

    pub fn leftJoinLateral(self, query: &Builder<'_>, alias: &str) -> DatabaseResult<Self> {
        self.join_lateral(query, alias, true)
    }

    fn join_lateral(self, query: &Builder<'_>, alias: &str, left: bool) -> DatabaseResult<Self> {
        let (operator, condition) = self.grammar.lateral_join(left)?;
        self.join_sub(
            query,
            alias,
            JoinType::Lateral {
                operator,
                condition,
            },
            |join| join,
        )
    }

    fn join_sub(
        self,
        query: &Builder<'_>,
        alias: &str,
        kind: JoinType,
        configure: impl FnOnce(JoinClause) -> JoinClause,
    ) -> DatabaseResult<Self> {
        let expression = self.subquery_expression(query)?;
        Ok(self.add_join(
            Source::Query(expression, alias.into()),
            kind,
            configure(JoinClause::default()),
        ))
    }

    fn add_join(mut self, source: Source, kind: JoinType, clause: JoinClause) -> Self {
        self.joins.push(Join {
            source,
            kind,
            clause,
        });
        self
    }

    pub(in crate::database) fn related_count_expression(
        &self,
        query: &Builder<'_>,
        operator: &str,
        count: u64,
    ) -> DatabaseResult<Expression> {
        let (operator, count) = related_count_comparison(operator, count)?;
        let query = self.subquery_expression(query)?;
        let mut bindings = query.bindings;
        bindings.push(count.into());
        Ok(Expression::new(format!("({}) {operator} ?", query.sql)).with_bindings(bindings))
    }

    pub(in crate::database) fn and_expression(mut self, expression: Expression) -> Self {
        self.predicates.raw(false, expression);
        self
    }

    pub(in crate::database) fn morph_count_expression(
        &self,
        column: &str,
        queries: Vec<(&str, Builder<'_>)>,
        operator: &str,
        count: u64,
        include_null: bool,
        negated: bool,
    ) -> DatabaseResult<Expression> {
        let (operator, count) = related_count_comparison(operator, count)?;
        let column = self.grammar.column(column)?;
        let mut bindings = Vec::new();
        let mut branches = Vec::with_capacity(queries.len());
        for (alias, query) in queries {
            bindings.push(alias.into());
            let use_exists = count == 1 && matches!(operator, ">=" | "<");
            let query = if use_exists {
                query.reorder()
            } else {
                query.select_related_aggregate("count", "*")?
            };
            let query = self.subquery_expression(&query)?;
            bindings.extend(query.bindings);
            let comparison = if use_exists {
                let prefix = if operator == "<" {
                    "not exists"
                } else {
                    "exists"
                };
                format!("{prefix} ({})", query.sql)
            } else {
                bindings.push(count.into());
                format!("({}) {operator} ?", query.sql)
            };
            branches.push(format!("({column} = ? and {comparison})"));
        }
        let predicate = if branches.is_empty() {
            bindings.push(count.into());
            format!("0 {operator} ?")
        } else {
            if include_null {
                branches.push(format!("{column} is null"));
            }
            format!("({})", branches.join(" or "))
        };
        let predicate = if negated {
            format!("not coalesce(({predicate}), 0)")
        } else {
            predicate
        };
        Ok(Expression::new(predicate).with_bindings(bindings))
    }

    pub fn whereExists(self, query: &Builder<'_>) -> DatabaseResult<Self> {
        self.where_exists(query, false, false)
    }

    pub fn orWhereExists(self, query: &Builder<'_>) -> DatabaseResult<Self> {
        self.where_exists(query, true, false)
    }

    pub fn whereNotExists(self, query: &Builder<'_>) -> DatabaseResult<Self> {
        self.where_exists(query, false, true)
    }

    pub fn orWhereNotExists(self, query: &Builder<'_>) -> DatabaseResult<Self> {
        self.where_exists(query, true, true)
    }

    fn where_exists(
        mut self,
        query: &Builder<'_>,
        or: bool,
        negated: bool,
    ) -> DatabaseResult<Self> {
        let expression = self.exists_expression(query, negated)?;
        self.predicates.raw(or, expression);
        Ok(self)
    }

    pub(in crate::database) fn exists_expression(
        &self,
        query: &Builder<'_>,
        negated: bool,
    ) -> DatabaseResult<Expression> {
        let query = self.subquery_expression(query)?;
        let operator = if negated { "not exists" } else { "exists" };
        Ok(Expression::new(format!("{operator} ({})", query.sql)).with_bindings(query.bindings))
    }

    pub fn whereInSub(self, column: &str, query: &Builder<'_>) -> DatabaseResult<Self> {
        self.where_in_sub(column, query, false)
    }

    pub fn whereNotInSub(self, column: &str, query: &Builder<'_>) -> DatabaseResult<Self> {
        self.where_in_sub(column, query, true)
    }

    fn where_in_sub(
        mut self,
        column: &str,
        query: &Builder<'_>,
        negated: bool,
    ) -> DatabaseResult<Self> {
        let query = self.subquery_expression(query)?;
        let column = self.grammar.column(column)?;
        let operator = if negated { "not in" } else { "in" };
        self.predicates.raw(
            false,
            Expression::new(format!("{column} {operator} ({})", query.sql))
                .with_bindings(query.bindings),
        );
        Ok(self)
    }

    pub fn whereSub(
        mut self,
        column: &str,
        operator: &str,
        query: &Builder<'_>,
    ) -> DatabaseResult<Self> {
        let query = self.subquery_expression(query)?;
        let column = self.grammar.column(column)?;
        let operator = crate::database::query::conditions::operator(operator, &self.grammar)?;
        self.predicates.raw(
            false,
            Expression::new(format!("{column} {operator} ({})", query.sql))
                .with_bindings(query.bindings),
        );
        Ok(self)
    }

    pub fn union(self, query: &Builder<'_>) -> DatabaseResult<Self> {
        self.add_union(query, false)
    }

    pub fn unionAll(self, query: &Builder<'_>) -> DatabaseResult<Self> {
        self.add_union(query, true)
    }

    fn add_union(mut self, query: &Builder<'_>, all: bool) -> DatabaseResult<Self> {
        let query = self.subquery_expression(query)?;
        self.unions.push((all, query));
        Ok(self)
    }

    pub(super) fn compile_unions(
        &self,
        sql: String,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<String> {
        let mut sql = self.grammar.wrap_union(&sql);
        for (all, query) in &self.unions {
            sql.push_str(if *all { " union all " } else { " union " });
            sql.push_str(&self.grammar.wrap_union(&query.sql));
            bindings.extend_from_slice(&query.bindings);
        }
        self.compile_orders(&self.union_orders, &mut sql, bindings)?;
        Grammar::pagination(&mut sql, self.union_limit, self.union_offset);
        Ok(sql)
    }

    pub(super) fn active_orders(&mut self) -> &mut Vec<Order> {
        if self.unions.is_empty() {
            &mut self.orders
        } else {
            &mut self.union_orders
        }
    }
}

fn related_count_comparison(operator: &str, count: u64) -> DatabaseResult<(&str, i64)> {
    let operator = match operator {
        "=" | "!=" | "<>" | "<" | "<=" | ">" | ">=" => operator,
        _ => return Err(DatabaseError::InvalidQuery("relationship count operator")),
    };
    let count = i64::try_from(count)
        .map_err(|_| DatabaseError::InvalidQuery("relationship count range"))?;
    Ok((operator, count))
}

//! SQLite 受限写目标，使用 rowid 子查询和现有双绑定执行计划。

use super::{Alias, Expr, ExprTrait, Query, QueryState, Result, SelectStatement, Values};

impl<'tx: 'cap, 'cap> QueryState<'tx, 'cap> {
    fn sqlite_rowid_target(&self) -> bool {
        self.has_joins || self.limit.is_some()
    }

    fn sqlite_target_select(&self, bindings: bool) -> Result<SelectStatement> {
        // 只替换根投影；保留join、where、排序、分页和union分支及冻结绑定。
        // 字面rowid可被用户列遮蔽；WITHOUT ROWID缺该列时由SQLite原生报错，不偷偷改用主键。
        let mut target = self.clone();
        let rowid = Expr::col((Alias::new(self.required_table()?), Alias::new("rowid")));
        target.select.clear_selects().expr(rowid.clone());
        target.projection = Some(vec![rowid]);
        target.scoped_select(bindings)
    }

    fn sqlite_finish_write(&self, compiled: (String, Values)) -> Result<(String, Values)> {
        if !self.sqlite_rowid_target() {
            // PDO拒绝直接写SQL未使用的非select绑定；SQLx忽略多余参数，必须执行前拒绝。
            // SeaQuery的分页参数不是PHP业务绑定，必须从根WHERE和完整查询同时排除。
            let mut source = self.clone();
            source.select.clear_selects();
            source.projection = Some(Vec::new());
            let (_, all) = source.scoped_select(true)?.build(SqliteWriteBuilder {
                omit_pagination: true,
            });
            let mut predicates = Query::select();
            predicates.expr(Expr::cust("1"));
            predicates.cond_where(self.predicates.compile(&self.context(), true)?);
            let (_, where_values) = predicates.build(SqliteWriteBuilder {
                omit_pagination: true,
            });
            if all.0.len() > where_values.0.len() {
                return Err(crate::Error::custom(
                    "query_sqlite_unused_write_bindings",
                    "SQLite直接写语句存在未使用的非select业务绑定",
                ));
            }
        }
        Ok(compiled)
    }

    pub(super) fn sqlite_compile_update(
        &self,
        values: &[(Alias, Expr)],
        bindings: bool,
    ) -> Result<(String, Values)> {
        self.validate()?;
        let mut statement = Query::update();
        statement
            .table(Alias::new(self.required_table()?))
            .values(values.to_vec());
        if self.sqlite_rowid_target() {
            statement.and_where(
                Expr::col(Alias::new("rowid")).in_subquery(self.sqlite_target_select(bindings)?),
            );
        } else {
            statement.cond_where(self.predicates.compile(&self.context(), bindings)?);
        }
        self.sqlite_finish_write(statement.build(SqliteWriteBuilder::default()))
    }

    pub(super) fn sqlite_compile_delete(&self, bindings: bool) -> Result<(String, Values)> {
        self.validate()?;
        let mut statement = Query::delete();
        statement.from_table(Alias::new(self.required_table()?));
        if self.sqlite_rowid_target() {
            statement.and_where(
                Expr::col(Alias::new("rowid")).in_subquery(self.sqlite_target_select(bindings)?),
            );
        } else {
            statement.cond_where(self.predicates.compile(&self.context(), bindings)?);
        }
        self.sqlite_finish_write(statement.build(SqliteWriteBuilder::default()))
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "sqlite_write_target_tests.rs"]
mod tests;

// 仅用于SQLite写SQL；默认遍历保留self，使任意深度的UNION分支使用相同隔离规则。
#[derive(Default)]
struct SqliteWriteBuilder {
    omit_pagination: bool,
}
use sea_orm::sea_query::{
    Asterisk, BinOper, EscapeBuilder, Oper, OperLeftAssocDecider, PrecedenceDecider, QueryBuilder,
    Quote, QuotedBuilder, SqlWriter, SqliteQueryBuilder, TableRefBuilder, UnionType,
};
impl QuotedBuilder for SqliteWriteBuilder {
    fn quote(&self) -> Quote {
        SqliteQueryBuilder.quote()
    }
}
impl EscapeBuilder for SqliteWriteBuilder {
    fn write_escaped(&self, buffer: &mut impl std::fmt::Write, string: &str) {
        SqliteQueryBuilder.write_escaped(buffer, string);
    }
    fn unescape_string(&self, string: &str) -> String {
        SqliteQueryBuilder.unescape_string(string)
    }
}
impl TableRefBuilder for SqliteWriteBuilder {}
impl PrecedenceDecider for SqliteWriteBuilder {
    fn inner_expr_well_known_greater_precedence(&self, inner: &Expr, outer_oper: &Oper) -> bool {
        SqliteQueryBuilder.inner_expr_well_known_greater_precedence(inner, outer_oper)
    }
}
impl OperLeftAssocDecider for SqliteWriteBuilder {
    fn well_known_left_associative(&self, op: &BinOper) -> bool {
        SqliteQueryBuilder.well_known_left_associative(op)
    }
}
impl QueryBuilder for SqliteWriteBuilder {
    fn prepare_explain_statement(
        &self,
        explain: &sea_orm::sea_query::ExplainStatement,
        sql: &mut impl SqlWriter,
    ) {
        SqliteQueryBuilder.prepare_explain_statement(explain, sql);
    }
    fn prepare_union_statement(
        &self,
        kind: UnionType,
        branch: &SelectStatement,
        sql: &mut impl SqlWriter,
    ) {
        // 以AST构造select * from(branch)，不拼接SQL或改动分支绑定。
        // 格式写入使用SeaQuery的SqlWriter，与其余方言方法相同。
        let mut wrapper = Query::select();
        wrapper
            .column(Asterisk)
            .from_subquery(branch.clone(), Alias::new("sqlite_union_branch"));
        let _ = sql.write_str(match kind {
            UnionType::All => " UNION ALL ",
            UnionType::Distinct => " UNION ",
            UnionType::Intersect => " INTERSECT ",
            UnionType::Except => " EXCEPT ",
            _ => {
                SqliteQueryBuilder.prepare_union_statement(kind, &wrapper, sql);
                return;
            }
        });
        self.prepare_select_statement(&wrapper, sql);
    }
    fn prepare_query_statement(
        &self,
        query: &sea_orm::sea_query::SubQueryStatement,
        sql: &mut impl SqlWriter,
    ) {
        use sea_orm::sea_query::SubQueryStatement as SubQuery;
        match query {
            SubQuery::SelectStatement(stmt) => self.prepare_select_statement(stmt, sql),
            SubQuery::InsertStatement(stmt) => self.prepare_insert_statement(stmt, sql),
            SubQuery::UpdateStatement(stmt) => self.prepare_update_statement(stmt, sql),
            SubQuery::DeleteStatement(stmt) => self.prepare_delete_statement(stmt, sql),
            SubQuery::WithStatement(stmt) => self.prepare_with_query(stmt, sql),
        }
    }
    fn prepare_select_limit_offset(&self, select: &SelectStatement, sql: &mut impl SqlWriter) {
        if !self.omit_pagination {
            SqliteQueryBuilder.prepare_select_limit_offset(select, sql);
        }
    }
    fn prepare_select_lock(
        &self,
        value: &sea_orm::sea_query::LockClause,
        sql: &mut impl SqlWriter,
    ) {
        SqliteQueryBuilder.prepare_select_lock(value, sql);
    }
    fn prepare_select_into(
        &self,
        value: &sea_orm::sea_query::SelectInto,
        sql: &mut impl SqlWriter,
    ) {
        SqliteQueryBuilder.prepare_select_into(value, sql);
    }
    fn prepare_sub_query_oper(
        &self,
        value: &sea_orm::sea_query::SubQueryOper,
        sql: &mut impl SqlWriter,
    ) {
        SqliteQueryBuilder.prepare_sub_query_oper(value, sql);
    }
    fn prepare_bin_oper(&self, value: &BinOper, sql: &mut impl SqlWriter) {
        SqliteQueryBuilder.prepare_bin_oper(value, sql);
    }
    fn prepare_with_clause_recursive_options(
        &self,
        value: &sea_orm::sea_query::WithClause,
        sql: &mut impl SqlWriter,
    ) {
        SqliteQueryBuilder.prepare_with_clause_recursive_options(value, sql);
    }
    fn prepare_value(&self, value: sea_orm::sea_query::Value, sql: &mut impl SqlWriter) {
        sql.push_param(value, self as _);
    }
    fn greatest_function(&self) -> &'static str {
        "MAX"
    }
    fn least_function(&self) -> &'static str {
        "MIN"
    }
    fn char_length_function(&self) -> &'static str {
        "LENGTH"
    }
    fn insert_default_values(&self, count: u32, sql: &mut impl SqlWriter) {
        SqliteQueryBuilder.insert_default_values(count, sql);
    }
    fn prepare_order_expr(&self, order: &sea_orm::sea_query::OrderExpr, sql: &mut impl SqlWriter) {
        // 当前公开排序只构造Alias列与Asc/Desc；保留SQLite专用NULLS/FIELD方言。
        SqliteQueryBuilder.prepare_order_expr(order, sql);
    }
}

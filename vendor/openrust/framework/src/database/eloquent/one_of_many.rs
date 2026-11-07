//! 在数据库按分组及指定聚合挑选关联行。

use std::fmt::Write as _;

use crate::{
    Db, Result,
    database::{DatabaseTableNames, DatabaseTransaction, QueryBuilder, RawSql, WhereGroup},
};

/// 单值关系在当前分组内选取的聚合方向。
#[derive(Clone, Copy)]
pub enum RelationAggregate {
    /// 选择最小值。
    Min,
    /// 选择最大值。
    Max,
}

impl RelationAggregate {
    const fn sql(self) -> &'static str {
        match self {
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }
}

pub(super) type CandidateFilter<'a> =
    dyn for<'q> Fn(WhereGroup<'q, 'q>) -> Result<WhereGroup<'q, 'q>> + Send + Sync + 'a;

pub(super) fn constrain<'q>(
    db: &Db,
    table: &str,
    foreign_key: &str,
    columns: &[(&str, RelationAggregate)],
    mut query: QueryBuilder<'q, 'q>,
    transaction: Option<&'q DatabaseTransaction>,
    filter: Option<&CandidateFilter<'_>>,
) -> Result<QueryBuilder<'q, 'q>> {
    let backend = db.backend()?;
    let identifiers = DatabaseTableNames::default();
    let physical = db.table_name(table)?;
    let outer = db.quoted_table_name(table)?;
    let alias = if physical.eq_ignore_ascii_case("openrust_relation_aggregate") {
        "openrust_relation_candidates"
    } else {
        "openrust_relation_aggregate"
    };
    let quoted_alias = identifiers.quote(alias, backend)?;
    let foreign_key = identifiers.quote(foreign_key, backend)?;
    let mut group = format!("{quoted_alias}.{foreign_key} = {outer}.{foreign_key}");
    for (column, aggregate) in columns {
        let quoted_column = identifiers.quote(column, backend)?;
        let aggregate = aggregate.sql();
        let subquery = db.table(table)?.alias_relation_table(alias)?;
        let subquery = match transaction {
            Some(transaction) => subquery.using_transaction(transaction),
            None => subquery,
        };
        let subquery = subquery
            .select_raw(
                RawSql::new(format!("{aggregate}({quoted_alias}.{quoted_column})")),
                Vec::new(),
            )?
            .where_raw(RawSql::new(group.clone()), Vec::new());
        let subquery = match filter {
            Some(filter) => subquery.where_nested(filter)?,
            None => subquery,
        };
        query = query.r#where((*column).to_owned(), subquery)?;
        write!(
            group,
            " AND {quoted_alias}.{quoted_column} = {outer}.{quoted_column}"
        )
        .map_err(crate::Error::internal)?;
    }
    Ok(query)
}

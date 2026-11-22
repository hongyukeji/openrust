//! SQL query construction and execution.

mod builder;
mod conditions;
pub mod embeddings;
mod expression;
pub mod grammars;
mod join_clause;
pub mod processors;
pub(super) mod vector;

pub use builder::{Builder, Column};
pub(super) use builder::{
    ChunkKey, collection_chunk_index, flatten_batches, map_chunk_rows, visit_batches,
};
pub use conditions::{
    Conditions, FullTextOptions, JsonOperand, PostgresFullTextMode, ValueOperand,
};
pub use expression::Expression;
pub use join_clause::JoinClause;

/// Named column values for an insert or update.
pub type Record = std::collections::BTreeMap<String, crate::database::Value>;

/// An ordinary value, a trusted expression, or a scalar query for an update assignment.
pub enum UpdateValue<'query, 'connection> {
    Operand(ValueOperand),
    Subquery(&'query mut Builder<'connection>),
}

impl<T: Into<ValueOperand>> From<T> for UpdateValue<'_, '_> {
    fn from(value: T) -> Self {
        Self::Operand(value.into())
    }
}

/// A pessimistic lock mode or an application-defined SQL locking clause.
#[derive(Clone)]
pub enum QueryLock {
    Exclusive,
    Shared,
    Raw(String),
}

impl From<bool> for QueryLock {
    fn from(exclusive: bool) -> Self {
        if exclusive {
            Self::Exclusive
        } else {
            Self::Shared
        }
    }
}

impl From<&str> for QueryLock {
    fn from(sql: &str) -> Self {
        Self::Raw(sql.to_owned())
    }
}

impl From<String> for QueryLock {
    fn from(sql: String) -> Self {
        Self::Raw(sql)
    }
}

/// The value assigned to a column when an upsert finds a conflict.
pub enum UpsertUpdate<'a> {
    /// Copy the named column from the attempted insert.
    FromInsert(&'a str),
    /// Bind a value for this column regardless of the attempted insert.
    Value(&'a str, crate::database::Value),
    /// Evaluate a trusted SQL expression for this column.
    Expression(&'a str, Expression),
}

/// 存在性条件已满足，或条件不满足时执行回调取得的结果。
#[derive(Debug, PartialEq, Eq)]
pub enum ExistsOr<T> {
    Satisfied,
    Callback(T),
}

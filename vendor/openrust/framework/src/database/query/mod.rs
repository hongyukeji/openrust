//! SQL query construction and execution.

mod builder;
mod conditions;
mod expression;
pub mod grammars;
mod join_clause;
pub mod processors;

pub use builder::{Builder, Column};
pub(super) use builder::{
    ChunkKey, collection_chunk_index, flatten_batches, map_chunk_rows, visit_batches,
};
pub use conditions::{Conditions, FullTextOptions, JsonOperand};
pub use expression::Expression;
pub use join_clause::JoinClause;

/// Named column values for an insert or update.
pub type Record = std::collections::BTreeMap<String, crate::database::Value>;

/// The value assigned to a column when an upsert finds a conflict.
pub enum UpsertUpdate<'a> {
    /// Copy the named column from the attempted insert.
    FromInsert(&'a str),
    /// Bind a value for this column regardless of the attempted insert.
    Value(&'a str, crate::database::Value),
    /// Evaluate a trusted SQL expression for this column.
    Expression(&'a str, Expression),
}

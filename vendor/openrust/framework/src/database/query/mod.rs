//! SQL query construction and execution.

mod builder;
mod conditions;
mod expression;
pub mod grammars;
mod join_clause;
pub mod processors;

pub use builder::{Builder, Column};
pub use conditions::Conditions;
pub use expression::Expression;
pub use join_clause::JoinClause;

/// Named column values for an insert or update.
pub struct Record {
    _private: (),
}

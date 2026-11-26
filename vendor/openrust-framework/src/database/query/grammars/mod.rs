//! SQL compilation by database dialect.

mod grammar;
pub(crate) use grammar::Grammar;
pub(super) use grammar::{DatePart, IndexHintKind};

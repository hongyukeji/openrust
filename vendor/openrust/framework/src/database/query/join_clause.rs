use crate::database::Value;

use super::Conditions;

/// The column comparisons and bound values of a join.
#[derive(Clone, Default)]
pub struct JoinClause(pub(super) Conditions);

impl JoinClause {
    pub fn on(self, column: &str, operator: &str, other: &str) -> Self {
        Self(self.0.where_column(column, operator, other))
    }

    pub fn or_on(self, column: &str, operator: &str, other: &str) -> Self {
        Self(self.0.or_where_column(column, operator, other))
    }

    pub fn r#where(self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        Self(self.0.r#where(column, operator, value))
    }

    pub fn or_where(self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        Self(self.0.or_where(column, operator, value))
    }

    pub fn where_null(self, column: &str) -> Self {
        Self(self.0.where_null(column))
    }

    pub fn on_nested(self, configure: impl FnOnce(Self) -> Self) -> Self {
        Self(
            self.0
                .where_nested(|conditions| configure(Self(conditions)).0),
        )
    }
}

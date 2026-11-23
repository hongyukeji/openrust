use crate::database::Value;

use super::Conditions;

/// The column comparisons and bound values of a join.
#[derive(Clone, Default)]
pub struct JoinClause(pub(super) Conditions);

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl JoinClause {
    pub fn on(self, column: &str, operator: &str, other: &str) -> Self {
        Self(self.0.whereColumn(column, operator, other))
    }

    pub fn orOn(self, column: &str, operator: &str, other: &str) -> Self {
        Self(self.0.orWhereColumn(column, operator, other))
    }

    pub fn r#where(self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        Self(self.0.r#where(column, operator, value))
    }

    pub fn orWhere(self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        Self(self.0.orWhere(column, operator, value))
    }

    pub fn whereNull(self, column: &str) -> Self {
        Self(self.0.whereNull(column))
    }

    pub fn onNested(self, configure: impl FnOnce(Self) -> Self) -> Self {
        Self(
            self.0
                .whereNested(|conditions| configure(Self(conditions)).0),
        )
    }
}

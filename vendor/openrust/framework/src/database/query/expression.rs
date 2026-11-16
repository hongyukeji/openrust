/// A trusted SQL expression with separately bound parameters.
#[derive(Clone)]
pub struct Expression {
    pub(super) sql: String,
    pub(super) bindings: Vec<crate::database::Value>,
}

impl Expression {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            bindings: Vec::new(),
        }
    }

    pub(in crate::database) fn unbound_sql(&self) -> Option<&str> {
        self.bindings.is_empty().then_some(&self.sql)
    }

    pub fn with_bindings(mut self, bindings: Vec<crate::database::Value>) -> Self {
        self.bindings = bindings;
        self
    }
}

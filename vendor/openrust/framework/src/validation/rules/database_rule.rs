use std::sync::Arc;

use crate::database::{
    Value,
    eloquent::Model,
    query::{Builder, Conditions},
};

use crate::validation::{ValidationError, ValidationResult};

type Callback = Arc<dyn Fn(Conditions) -> Conditions + Send + Sync>;

#[derive(Clone)]
enum Condition {
    Compare(String, &'static str, Value),
    In(String, Vec<Value>, bool),
    Callback(Callback),
}

#[derive(Clone)]
pub(in crate::validation) struct DatabaseRule {
    pub connection: Option<String>,
    pub table: String,
    pub column: Option<String>,
    conditions: Vec<Condition>,
}

impl DatabaseRule {
    pub(super) fn new(table: &str, column: Option<&str>) -> Self {
        let (connection, table) = table
            .split_once('.')
            .map_or((None, table), |(connection, table)| {
                (Some(connection.to_owned()), table)
            });
        Self {
            connection,
            table: table.to_owned(),
            column: column.filter(|column| *column != "NULL").map(str::to_owned),
            conditions: Vec::new(),
        }
    }

    pub(super) fn model(model: &impl Model, column: Option<&str>) -> Self {
        Self {
            connection: model.getConnectionName().map(str::to_owned),
            table: model.getTable().to_owned(),
            column: column.map(str::to_owned),
            conditions: Vec::new(),
        }
    }

    pub(super) fn compare(&mut self, column: &str, operator: &'static str, value: Value) {
        self.conditions
            .push(Condition::Compare(column.to_owned(), operator, value));
    }

    pub(super) fn in_set(&mut self, column: &str, values: Vec<Value>, negate: bool) {
        self.conditions
            .push(Condition::In(column.to_owned(), values, negate));
    }

    pub(super) fn callback(
        &mut self,
        callback: impl Fn(Conditions) -> Conditions + Send + Sync + 'static,
    ) {
        self.conditions
            .push(Condition::Callback(Arc::new(callback)));
    }

    pub(super) fn parse_conditions(&mut self, values: &[&str]) -> ValidationResult<()> {
        let pairs = values.chunks_exact(2);
        if !pairs.remainder().is_empty() {
            return Err(ValidationError::InvalidRule);
        }
        // String rules use the last value supplied for a repeated condition column.
        for (index, pair) in pairs.enumerate() {
            let [column, value] = pair else {
                unreachable!()
            };
            if column.is_empty() {
                return Err(ValidationError::InvalidRule);
            }
            if values[(index + 1) * 2..]
                .iter()
                .step_by(2)
                .any(|later| later == column)
            {
                continue;
            }
            match *value {
                "NULL" => self.compare(column, "=", Value::Null),
                "NOT_NULL" => self.compare(column, "!=", Value::Null),
                value => {
                    let (operator, value) = value
                        .strip_prefix('!')
                        .map_or(("=", value), |value| ("!=", value));
                    self.compare(column, operator, value.into());
                }
            }
        }
        Ok(())
    }

    pub(in crate::validation) fn apply(&self, query: Builder<'static>) -> Builder<'static> {
        self.conditions
            .iter()
            .fold(query, |query, condition| match condition {
                Condition::Compare(column, "=", Value::Null) => query.where_null(column),
                Condition::Compare(column, "!=", Value::Null) => query.where_not_null(column),
                Condition::Compare(column, operator, value) => {
                    query.r#where(column, operator, value.clone())
                }
                Condition::In(column, values, false) => query.where_in(column, values.clone()),
                Condition::In(column, values, true) => query.where_not_in(column, values.clone()),
                Condition::Callback(callback) => {
                    query.whereNested(|conditions| callback(conditions))
                }
            })
    }
}

macro_rules! database_conditions {
    () => {
        pub fn r#where(mut self, column: &str, value: impl Into<Value>) -> Self {
            self.target.compare(column, "=", value.into());
            self
        }

        pub fn where_not(mut self, column: &str, value: impl Into<Value>) -> Self {
            self.target.compare(column, "!=", value.into());
            self
        }

        pub fn where_null(self, column: &str) -> Self {
            self.r#where(column, Value::Null)
        }

        pub fn where_not_null(self, column: &str) -> Self {
            self.where_not(column, Value::Null)
        }

        pub fn where_in(mut self, column: &str, values: Vec<Value>) -> Self {
            self.target.in_set(column, values, false);
            self
        }

        pub fn where_not_in(mut self, column: &str, values: Vec<Value>) -> Self {
            self.target.in_set(column, values, true);
            self
        }

        pub fn without_trashed(self, column: Option<&str>) -> Self {
            self.where_null(column.unwrap_or("deleted_at"))
        }

        pub fn only_trashed(self, column: Option<&str>) -> Self {
            self.where_not_null(column.unwrap_or("deleted_at"))
        }

        pub fn using(
            mut self,
            callback: impl Fn(Conditions) -> Conditions + Send + Sync + 'static,
        ) -> Self {
            self.target.callback(callback);
            self
        }
    };
}
pub(super) use database_conditions;

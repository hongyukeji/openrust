use crate::{
    database::{Value, eloquent::Model, query::Conditions},
    validation::{Rule, ValidationResult, rule::Kind},
};

use super::database_rule::{DatabaseRule, database_conditions};

/// Verify that a value is unique in a database table.
#[derive(Clone)]
pub struct Unique {
    pub(in crate::validation) target: DatabaseRule,
    pub(in crate::validation) exclusion: Option<Exclusion>,
    key_column: String,
}

#[derive(Clone)]
pub(in crate::validation) enum Exclusion {
    Value(String, Value),
    Input(String, String),
}

#[allow(non_snake_case, reason = "Matches Laravel's database validation rules")]
impl Unique {
    pub fn new(table: &str, column: Option<&str>) -> Self {
        Self {
            target: DatabaseRule::new(table, column),
            exclusion: None,
            key_column: "id".into(),
        }
    }

    pub fn fromModel(model: &impl Model, column: Option<&str>) -> Self {
        Self {
            target: DatabaseRule::model(model, column),
            exclusion: None,
            key_column: model.getKeyName().into(),
        }
    }

    pub fn ignore(mut self, id: impl Into<Value>, column: Option<&str>) -> Self {
        let id = id.into();
        self.exclusion = (!matches!(id, Value::Null))
            .then(|| Exclusion::Value(column.unwrap_or(&self.key_column).to_owned(), id));
        self
    }

    pub fn ignoreModel(self, model: &impl Model, column: Option<&str>) -> Self {
        let column = column.unwrap_or_else(|| model.getKeyName());
        self.ignore(
            model.getAttribute(column).cloned().unwrap_or(Value::Null),
            Some(column),
        )
    }

    database_conditions!();

    pub(in crate::validation) fn parse(table: &str, arguments: &[&str]) -> ValidationResult<Self> {
        let mut rule = Self::new(table, arguments.first().copied());
        if let Some(id) = arguments
            .get(1)
            .filter(|id| !id.eq_ignore_ascii_case("null"))
        {
            let column = arguments.get(2).copied().unwrap_or("id");
            if let Some(path) = id.strip_prefix('[').and_then(|id| id.strip_suffix(']')) {
                rule.exclusion = Some(Exclusion::Input(column.to_owned(), path.to_owned()));
            } else {
                let id = id
                    .parse::<i64>()
                    .map_or_else(|_| Value::from(*id), Value::Integer);
                rule = rule.ignore(id, Some(column));
            }
        }
        rule.target
            .parse_conditions(arguments.get(3..).unwrap_or_default())?;
        Ok(rule)
    }
}

impl From<Unique> for Rule {
    fn from(rule: Unique) -> Self {
        Rule(Kind::Unique(rule))
    }
}

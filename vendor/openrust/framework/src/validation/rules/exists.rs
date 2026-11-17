use crate::{
    database::{Value, eloquent::Model, query::Conditions},
    validation::{Rule, ValidationResult, rule::Kind},
};

use super::database_rule::{DatabaseRule, database_conditions};

/// Verify that a value or list of values exists in a database table.
#[derive(Clone)]
pub struct Exists {
    pub(in crate::validation) target: DatabaseRule,
}

#[allow(non_snake_case, reason = "Matches Laravel's database validation rules")]
impl Exists {
    pub fn new(table: &str, column: Option<&str>) -> Self {
        Self {
            target: DatabaseRule::new(table, column),
        }
    }

    pub fn fromModel(model: &impl Model, column: Option<&str>) -> Self {
        Self {
            target: DatabaseRule::model(model, column),
        }
    }

    database_conditions!();

    pub(in crate::validation) fn parse(table: &str, arguments: &[&str]) -> ValidationResult<Self> {
        let mut rule = Self::new(table, arguments.first().copied());
        rule.target
            .parse_conditions(arguments.get(1..).unwrap_or_default())?;
        Ok(rule)
    }
}

impl From<Exists> for Rule {
    fn from(rule: Exists) -> Self {
        Rule(Kind::Exists(rule))
    }
}

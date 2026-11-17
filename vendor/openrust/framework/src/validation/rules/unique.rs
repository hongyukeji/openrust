use crate::{
    database::{Value, eloquent::Model, query::Conditions},
    validation::{Rule, ValidationError, ValidationResult, path, rule::Kind},
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
        self.exclusion = (!matches!(id, Value::Null)
            && !matches!(&id, Value::Text(text) if text == "NULL"))
        .then(|| {
            Exclusion::Value(
                column
                    .filter(|value| !value.is_empty())
                    .unwrap_or(&self.key_column)
                    .to_owned(),
                id,
            )
        });
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
            let column = arguments
                .get(2)
                .copied()
                .filter(|column| !column.is_empty())
                .unwrap_or("id");
            if let Some((_, remainder)) = id.split_once('[')
                && let Some((attribute, _)) = remainder.rsplit_once(']')
                && !attribute.contains('\n')
            {
                if attribute.is_empty()
                    || path::parse(attribute)?
                        .iter()
                        .any(|segment| matches!(segment, path::Segment::Any))
                {
                    return Err(ValidationError::InvalidRule);
                }
                rule.exclusion = Some(Exclusion::Input(column.to_owned(), attribute.to_owned()));
            } else if let Some(id) = prepare_id(id) {
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

/// Normalize a string-rule ID using Laravel's integer and backslash handling.
pub(in crate::validation) fn prepare_id(id: &str) -> Option<Value> {
    if id.eq_ignore_ascii_case("null") {
        return None;
    }
    let number = id.trim_matches(|character: char| character.is_ascii_whitespace());
    let digits = number.strip_prefix(['+', '-']).unwrap_or(number);
    if !(digits.len() > 1 && digits.starts_with('0'))
        && let Ok(number) = number.parse::<i64>()
    {
        return Some(Value::Text(number.to_string()));
    }

    let mut chars = id.chars();
    let mut unescaped = String::with_capacity(id.len());
    while let Some(character) = chars.next() {
        if character == '\\' {
            unescaped.extend(
                chars
                    .next()
                    .map(|escaped| if escaped == '0' { '\0' } else { escaped }),
            );
        } else {
            unescaped.push(character);
        }
    }
    (unescaped != "NULL").then_some(Value::Text(unescaped))
}

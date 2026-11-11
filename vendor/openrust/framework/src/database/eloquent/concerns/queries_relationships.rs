use std::collections::HashSet;

use crate::database::{DatabaseError, DatabaseResult, Value};

use super::super::{Attributes, Builder, Model, relations::ExistenceRelation};

type ApplyAggregate<'a, M> =
    dyn Fn(Builder<'static, M>) -> DatabaseResult<Builder<'static, M>> + Send + Sync + 'a;

/// One typed relationship aggregate in a collection's deferred query.
pub struct RelationAggregateQuery<'a, M: Model> {
    name: &'a str,
    aggregate: RelationAggregate<'a>,
    apply: Box<ApplyAggregate<'a, M>>,
}

impl<'a, M: Model> RelationAggregateQuery<'a, M> {
    pub fn new(
        name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = M>,
        aggregate: RelationAggregate<'a>,
    ) -> Self {
        Self {
            name,
            aggregate,
            apply: Box::new(move |query| query.withAggregate(name, relation, aggregate)),
        }
    }

    pub(in crate::database::eloquent) fn alias(&self) -> DatabaseResult<String> {
        self.aggregate.alias(self.name)
    }

    pub(in crate::database::eloquent) fn apply(
        &self,
        builder: Builder<'static, M>,
    ) -> DatabaseResult<Builder<'static, M>> {
        (self.apply)(builder)
    }
}

/// An aggregate to select from a model's related rows.
#[derive(Clone, Copy)]
pub enum RelationAggregate<'a> {
    Count,
    Exists,
    Sum(&'a str),
    Avg(&'a str),
    Min(&'a str),
    Max(&'a str),
}

impl<'a> RelationAggregate<'a> {
    pub(in crate::database::eloquent) fn function(self) -> (&'static str, &'a str) {
        match self {
            Self::Count => ("count", "*"),
            Self::Exists => ("exists", "*"),
            Self::Sum(column) => ("sum", column),
            Self::Avg(column) => ("avg", column),
            Self::Min(column) => ("min", column),
            Self::Max(column) => ("max", column),
        }
    }

    pub(crate) fn alias(self, name: &str) -> DatabaseResult<String> {
        let words: Vec<_> = name.split_ascii_whitespace().collect();
        let (name, alias) = match words.as_slice() {
            [name] => (*name, None),
            [name, separator, alias] if separator.eq_ignore_ascii_case("as") => {
                (*name, Some(*alias))
            }
            _ => return Err(DatabaseError::InvalidQuery("relationship aggregate name")),
        };
        if !identifier(name) || alias.is_some_and(|alias| !identifier(alias)) {
            return Err(DatabaseError::InvalidQuery("relationship aggregate name"));
        }
        if let Some(alias) = alias {
            return Ok(alias.to_owned());
        }
        let (function, column) = self.function();
        let base = snake_case(name);
        if column == "*" {
            return Ok(format!("{base}_{function}"));
        }
        let column = column.rsplit('.').next().unwrap_or_default();
        if !identifier(column) {
            return Err(DatabaseError::InvalidQuery("relationship aggregate column"));
        }
        Ok(format!("{base}_{function}_{}", snake_case(column)))
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn snake_case(value: &str) -> String {
    let mut name = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte.is_ascii_uppercase()
            && index > 0
            && bytes[index - 1] != b'_'
            && (!bytes[index - 1].is_ascii_uppercase()
                || bytes.get(index + 1).is_some_and(u8::is_ascii_lowercase))
        {
            name.push('_');
        }
        name.push(char::from(byte.to_ascii_lowercase()));
    }
    name
}

pub(in crate::database::eloquent) fn cast_existence(
    attributes: &mut Attributes,
    columns: &HashSet<String>,
) {
    for alias in columns {
        let Some(value) = attributes.get_mut(alias) else {
            continue;
        };
        match value {
            Value::Integer(number) => *value = Value::Bool(*number != 0),
            Value::Unsigned(number) => *value = Value::Bool(*number != 0),
            _ => (),
        }
    }
}

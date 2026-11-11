use crate::database::{DatabaseError, DatabaseResult, Value};

use super::{
    Expression,
    grammars::{DatePart, Grammar},
};

/// The search mode for MySQL-compatible full-text indexes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FullTextOptions {
    #[default]
    NaturalLanguage,
    Boolean,
    QueryExpansion,
}

#[derive(Clone, Copy)]
enum Boolean {
    And,
    Or,
}

#[derive(Clone)]
enum Predicate {
    Value(String, String, Value),
    Column(String, String, String),
    Null(String, bool),
    In(String, Vec<Value>, bool),
    Between(String, [Value; 2], bool),
    Group(Conditions, bool),
    Like(String, String, bool, bool),
    FullText(Vec<String>, String, FullTextOptions),
    Date(DatePart, String, String, String),
    BetweenColumns(String, [String; 2], bool),
    Raw(Expression),
}

/// Predicates for a parenthesized where group.
#[derive(Clone, Default)]
pub struct Conditions(Vec<(Boolean, Predicate)>);

impl Conditions {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn compile(
        &self,
        grammar: &Grammar,
        sql: &mut String,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<()> {
        for (index, (boolean, predicate)) in self.0.iter().enumerate() {
            if index > 0 {
                sql.push_str(match boolean {
                    Boolean::And => " and ",
                    Boolean::Or => " or ",
                });
            }
            predicate.compile(grammar, sql, bindings)?;
        }
        Ok(())
    }
}

macro_rules! date_methods {
    ($(($and:ident, $or:ident, $part:ident, $value:ty, $convert:expr)),+ $(,)?) => {
        $(
            pub fn $and(mut self, column: &str, operator: &str, value: $value) -> Self {
                self.conditions().date(false, DatePart::$part, column, operator, $convert(value));
                self
            }

            pub fn $or(mut self, column: &str, operator: &str, value: $value) -> Self {
                self.conditions().date(true, DatePart::$part, column, operator, $convert(value));
                self
            }
        )+
    };
}

// Both a query and a nested group expose the same predicate methods.
macro_rules! predicate_methods {
    () => {
        date_methods! {
            (whereDate, orWhereDate, Date, chrono::NaiveDate, |value| format!("{value}")),
            (whereTime, orWhereTime, Time, chrono::NaiveTime, |value: chrono::NaiveTime| value.format("%H:%M:%S").to_string()),
            (whereDay, orWhereDay, Day, u32, |value| format!("{value:02}")),
            (whereMonth, orWhereMonth, Month, u32, |value| format!("{value:02}")),
            (whereYear, orWhereYear, Year, i32, |value| format!("{value:04}")),
        }

        pub fn whereBetweenColumns(mut self, column: &str, bounds: [&str; 2]) -> Self {
            self.conditions().between_columns(false, column, bounds, false);
            self
        }

        pub fn orWhereBetweenColumns(mut self, column: &str, bounds: [&str; 2]) -> Self {
            self.conditions().between_columns(true, column, bounds, false);
            self
        }

        pub fn whereNotBetweenColumns(mut self, column: &str, bounds: [&str; 2]) -> Self {
            self.conditions().between_columns(false, column, bounds, true);
            self
        }

        pub fn orWhereNotBetweenColumns(mut self, column: &str, bounds: [&str; 2]) -> Self {
            self.conditions().between_columns(true, column, bounds, true);
            self
        }

        pub fn r#where(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
            self.conditions()
                .compare(false, column, operator, value.into());
            self
        }

        pub fn orWhere(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
            self.conditions()
                .compare(true, column, operator, value.into());
            self
        }

        pub fn whereNullSafeEquals(self, column: &str, value: impl Into<Value>) -> Self {
            self.r#where(column, "<=>", value)
        }

        pub fn orWhereNullSafeEquals(self, column: &str, value: impl Into<Value>) -> Self {
            self.orWhere(column, "<=>", value)
        }

        pub fn whereLike(mut self, column: &str, pattern: &str, case_sensitive: bool) -> Self {
            self.conditions()
                .like(false, column, pattern, case_sensitive, false);
            self
        }

        pub fn orWhereLike(mut self, column: &str, pattern: &str, case_sensitive: bool) -> Self {
            self.conditions()
                .like(true, column, pattern, case_sensitive, false);
            self
        }

        pub fn whereNotLike(mut self, column: &str, pattern: &str, case_sensitive: bool) -> Self {
            self.conditions()
                .like(false, column, pattern, case_sensitive, true);
            self
        }

        pub fn orWhereNotLike(mut self, column: &str, pattern: &str, case_sensitive: bool) -> Self {
            self.conditions()
                .like(true, column, pattern, case_sensitive, true);
            self
        }

        pub fn whereFullText(self, columns: &[&str], value: &str) -> Self {
            self.whereFullTextWith(columns, value, $crate::database::query::FullTextOptions::default())
        }

        pub fn orWhereFullText(self, columns: &[&str], value: &str) -> Self {
            self.orWhereFullTextWith(columns, value, $crate::database::query::FullTextOptions::default())
        }

        pub fn whereFullTextWith(mut self, columns: &[&str], value: &str, options: $crate::database::query::FullTextOptions) -> Self {
            self.conditions().full_text(false, columns, value, options);
            self
        }

        pub fn orWhereFullTextWith(mut self, columns: &[&str], value: &str, options: $crate::database::query::FullTextOptions) -> Self {
            self.conditions().full_text(true, columns, value, options);
            self
        }

        pub fn whereAny(self, columns: &[&str], operator: &str, value: impl Into<Value>) -> Self {
            let value = value.into();
            self.whereNested(|group| {
                columns.iter().fold(group, |group, column| {
                    group.orWhere(column, operator, value.clone())
                })
            })
        }

        pub fn whereAll(self, columns: &[&str], operator: &str, value: impl Into<Value>) -> Self {
            let value = value.into();
            self.whereNested(|group| {
                columns.iter().fold(group, |group, column| {
                    group.r#where(column, operator, value.clone())
                })
            })
        }

        pub fn whereNone(self, columns: &[&str], operator: &str, value: impl Into<Value>) -> Self {
            let value = value.into();
            self.whereNot(|group| {
                columns.iter().fold(group, |group, column| {
                    group.orWhere(column, operator, value.clone())
                })
            })
        }

        pub fn whereColumn(mut self, column: &str, operator: &str, other: &str) -> Self {
            self.conditions()
                .compare_column(false, column, operator, other);
            self
        }

        pub fn orWhereColumn(mut self, column: &str, operator: &str, other: &str) -> Self {
            self.conditions()
                .compare_column(true, column, operator, other);
            self
        }

        pub fn whereNull(mut self, column: &str) -> Self {
            self.conditions().null(false, column, false);
            self
        }

        pub fn orWhereNull(mut self, column: &str) -> Self {
            self.conditions().null(true, column, false);
            self
        }

        pub fn whereNotNull(mut self, column: &str) -> Self {
            self.conditions().null(false, column, true);
            self
        }

        pub fn orWhereNotNull(mut self, column: &str) -> Self {
            self.conditions().null(true, column, true);
            self
        }

        pub fn whereIn(mut self, column: &str, values: Vec<Value>) -> Self {
            self.conditions().in_values(false, column, values, false);
            self
        }

        pub fn orWhereIn(mut self, column: &str, values: Vec<Value>) -> Self {
            self.conditions().in_values(true, column, values, false);
            self
        }

        pub fn whereNotIn(mut self, column: &str, values: Vec<Value>) -> Self {
            self.conditions().in_values(false, column, values, true);
            self
        }

        pub fn orWhereNotIn(mut self, column: &str, values: Vec<Value>) -> Self {
            self.conditions().in_values(true, column, values, true);
            self
        }

        pub fn whereBetween(mut self, column: &str, bounds: [Value; 2]) -> Self {
            self.conditions().between(false, column, bounds, false);
            self
        }

        pub fn orWhereBetween(mut self, column: &str, bounds: [Value; 2]) -> Self {
            self.conditions().between(true, column, bounds, false);
            self
        }

        pub fn whereNotBetween(mut self, column: &str, bounds: [Value; 2]) -> Self {
            self.conditions().between(false, column, bounds, true);
            self
        }

        pub fn orWhereNotBetween(mut self, column: &str, bounds: [Value; 2]) -> Self {
            self.conditions().between(true, column, bounds, true);
            self
        }

        pub fn whereNested(mut self, configure: impl FnOnce(Conditions) -> Conditions) -> Self {
            self.conditions().nested(false, configure, false);
            self
        }

        pub fn orWhereNested(mut self, configure: impl FnOnce(Conditions) -> Conditions) -> Self {
            self.conditions().nested(true, configure, false);
            self
        }

        pub fn whereNot(mut self, configure: impl FnOnce(Conditions) -> Conditions) -> Self {
            self.conditions().nested(false, configure, true);
            self
        }

        pub fn orWhereNot(mut self, configure: impl FnOnce(Conditions) -> Conditions) -> Self {
            self.conditions().nested(true, configure, true);
            self
        }

        pub fn whereRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
            self.conditions()
                .raw(false, Expression::new(sql).with_bindings(bindings));
            self
        }

        pub fn orWhereRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
            self.conditions()
                .raw(true, Expression::new(sql).with_bindings(bindings));
            self
        }
    };
}
pub(super) use {date_methods, predicate_methods};

#[allow(non_snake_case, reason = "Matches Laravel's Query Builder API")]
impl Conditions {
    fn conditions(&mut self) -> &mut Self {
        self
    }

    predicate_methods!();

    fn push(&mut self, or: bool, predicate: Predicate) {
        let boolean = if or { Boolean::Or } else { Boolean::And };
        self.0.push((boolean, predicate));
    }

    pub(super) fn compare(&mut self, or: bool, column: &str, operator: &str, value: Value) {
        self.push(or, Predicate::Value(column.into(), operator.into(), value));
    }

    pub(super) fn compare_column(&mut self, or: bool, column: &str, operator: &str, other: &str) {
        self.push(
            or,
            Predicate::Column(column.into(), operator.into(), other.into()),
        );
    }

    pub(super) fn null(&mut self, or: bool, column: &str, negated: bool) {
        self.push(or, Predicate::Null(column.into(), negated));
    }

    pub(super) fn in_values(&mut self, or: bool, column: &str, values: Vec<Value>, negated: bool) {
        self.push(or, Predicate::In(column.into(), values, negated));
    }

    pub(super) fn between(&mut self, or: bool, column: &str, bounds: [Value; 2], negated: bool) {
        self.push(or, Predicate::Between(column.into(), bounds, negated));
    }

    pub(super) fn nested(&mut self, or: bool, configure: impl FnOnce(Self) -> Self, negated: bool) {
        let group = configure(Self::default());
        if !group.is_empty() {
            self.push(or, Predicate::Group(group, negated));
        }
    }

    pub(super) fn like(
        &mut self,
        or: bool,
        column: &str,
        pattern: &str,
        case_sensitive: bool,
        negated: bool,
    ) {
        self.push(
            or,
            Predicate::Like(column.into(), pattern.into(), case_sensitive, negated),
        );
    }

    pub(super) fn full_text(
        &mut self,
        or: bool,
        columns: &[&str],
        value: &str,
        options: FullTextOptions,
    ) {
        self.push(
            or,
            Predicate::FullText(
                columns.iter().map(|column| (*column).to_owned()).collect(),
                value.to_owned(),
                options,
            ),
        );
    }

    pub(super) fn date(
        &mut self,
        or: bool,
        part: DatePart,
        column: &str,
        operator: &str,
        value: String,
    ) {
        self.push(
            or,
            Predicate::Date(part, column.into(), operator.into(), value),
        );
    }

    pub(super) fn between_columns(
        &mut self,
        or: bool,
        column: &str,
        bounds: [&str; 2],
        negated: bool,
    ) {
        self.push(
            or,
            Predicate::BetweenColumns(column.into(), bounds.map(str::to_owned), negated),
        );
    }

    pub(super) fn raw(&mut self, or: bool, expression: Expression) {
        self.push(or, Predicate::Raw(expression));
    }
}

pub(super) fn operator(value: &str, grammar: &Grammar) -> DatabaseResult<String> {
    let value = value.to_ascii_lowercase();
    match value.as_str() {
        "<=>" => Ok(grammar.null_safe_operator().into()),
        "=" | "!=" | "<>" | "<" | ">" | "<=" | ">=" | "like" | "not like" | "is" | "is not"
        | "&" | "|" | "<<" | ">>" | "regexp" | "not regexp" => Ok(value),
        _ => Err(DatabaseError::InvalidQuery("comparison operator")),
    }
}

impl Predicate {
    fn compile(
        &self,
        grammar: &Grammar,
        sql: &mut String,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<()> {
        match self {
            Self::Value(column, comparison, value) => {
                let operator = operator(comparison, grammar)?;
                if matches!(value, Value::Null) && comparison != "<=>" {
                    let negated = match operator.as_str() {
                        "=" | "is" => false,
                        "!=" | "<>" | "is not" => true,
                        _ => return Err(DatabaseError::InvalidQuery("operator for null value")),
                    };
                    return Self::Null(column.clone(), negated).compile(grammar, sql, bindings);
                }
                sql.push_str(&format!("{} {operator} ?", grammar.column(column)?));
                bindings.push(value.clone());
            }
            Self::Column(column, comparison, other) => {
                let operator = operator(comparison, grammar)?;
                sql.push_str(&format!(
                    "{} {operator} {}",
                    grammar.column(column)?,
                    grammar.column(other)?
                ));
            }
            Self::Null(column, negated) => {
                sql.push_str(&grammar.column(column)?);
                sql.push_str(if *negated { " is not null" } else { " is null" });
            }
            Self::In(_, values, negated) if values.is_empty() => {
                sql.push_str(if *negated { "1 = 1" } else { "0 = 1" });
            }
            Self::In(column, values, negated) => {
                sql.push_str(&grammar.column(column)?);
                sql.push_str(if *negated { " not in (" } else { " in (" });
                for index in 0..values.len() {
                    if index != 0 {
                        sql.push_str(", ");
                    }
                    sql.push('?');
                }
                sql.push(')');
                bindings.extend_from_slice(values);
            }
            Self::Between(column, bounds, negated) => {
                sql.push_str(&grammar.column(column)?);
                sql.push_str(if *negated {
                    " not between ? and ?"
                } else {
                    " between ? and ?"
                });
                bindings.extend_from_slice(bounds);
            }
            Self::Group(group, negated) => {
                sql.push_str(if *negated { "not (" } else { "(" });
                group.compile(grammar, sql, bindings)?;
                sql.push(')');
            }
            Self::Date(part, column, comparison, value) => {
                let operator = operator(comparison, grammar)?;
                sql.push_str(&grammar.date_part(*part, column, &operator)?);
                bindings.push(value.clone().into());
            }
            Self::BetweenColumns(column, bounds, negated) => {
                let operator = if *negated { "not between" } else { "between" };
                sql.push_str(&format!(
                    "{} {operator} {} and {}",
                    grammar.column(column)?,
                    grammar.column(&bounds[0])?,
                    grammar.column(&bounds[1])?
                ));
            }
            Self::Like(column, pattern, case_sensitive, negated) => {
                let (operator, pattern) = grammar.like(pattern, *case_sensitive);
                let not = if *negated { " not " } else { " " };
                sql.push_str(&format!("{}{not}{operator} ?", grammar.column(column)?));
                bindings.push(pattern.into());
            }
            Self::FullText(columns, value, options) => {
                sql.push_str(&grammar.full_text(columns, *options)?);
                bindings.push(value.clone().into());
            }
            Self::Raw(expression) => {
                sql.push_str(&expression.sql);
                bindings.extend_from_slice(&expression.bindings);
            }
        }
        Ok(())
    }
}

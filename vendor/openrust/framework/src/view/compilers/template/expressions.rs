use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::{ViewData, ViewResult, ViewValue, identifier, invalid, separators};

#[derive(Serialize, Deserialize)]
pub(super) enum Expression {
    Value(ViewValue),
    Variable(Vec<String>),
    Not(Box<Expression>),
    Binary(BinaryOperator, Box<Expression>, Box<Expression>),
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum BinaryOperator {
    Coalesce,
    Or,
    And,
    StrictEqual,
    StrictNotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

pub(super) fn expression_value(expression: &str) -> ViewResult<Expression> {
    parse_expression(expression, 0)
}

fn parse_expression(expression: &str, depth: usize) -> ViewResult<Expression> {
    if depth > 48 {
        return Err(invalid("Blade expressions are nested too deeply."));
    }
    let expression = expression.trim();
    if let Ok(value) = serde_json::from_str(expression) {
        return Ok(Expression::Value(value));
    }
    for operators in [
        [("??", BinaryOperator::Coalesce)].as_slice(),
        &[("||", BinaryOperator::Or)],
        &[("&&", BinaryOperator::And)],
        &[
            ("===", BinaryOperator::StrictEqual),
            ("!==", BinaryOperator::StrictNotEqual),
        ],
        &[
            ("<", BinaryOperator::Less),
            ("<=", BinaryOperator::LessOrEqual),
            (">", BinaryOperator::Greater),
            (">=", BinaryOperator::GreaterOrEqual),
        ],
    ] {
        let operation = operators
            .iter()
            .filter_map(|(token, operator)| {
                let mut indices = separators(expression, token).filter(|index| {
                    (!token.starts_with('>') || !expression[..*index].ends_with('-'))
                        && (token.len() != 1 || !expression[*index + 1..].starts_with('='))
                });
                let index = if matches!(operator, BinaryOperator::Coalesce) {
                    indices.next()
                } else {
                    indices.last()
                }?;
                Some((index, token.len(), *operator))
            })
            .max_by_key(|(index, _, _)| *index);
        if let Some((index, length, operator)) = operation {
            let lhs = parse_expression(&expression[..index], depth + 1)?;
            let rhs = parse_expression(&expression[index + length..], depth + 1)?;
            return Ok(Expression::Binary(operator, Box::new(lhs), Box::new(rhs)));
        }
    }
    if let Some(rest) = expression.strip_prefix('!') {
        return Ok(Expression::Not(Box::new(parse_expression(
            rest,
            depth + 1,
        )?)));
    }
    if let Some(group) = expression
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return parse_expression(group, depth + 1);
    }
    if let Some(literal) = expression
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        && !literal.contains(['\'', '\\'])
    {
        return Ok(Expression::Value(ViewValue::String(literal.to_owned())));
    }
    if let Some(path) = expression.strip_prefix('$') {
        let path: Vec<_> = path.split("->").map(str::trim).collect();
        if path.iter().all(|name| identifier(name)) {
            return Ok(Expression::Variable(
                path.into_iter().map(str::to_owned).collect(),
            ));
        }
    }
    Err(invalid("Unsupported Blade expression."))
}

pub(super) fn value<'a>(
    expression: &'a Expression,
    data: &'a ViewData,
    bindings: &[(&str, &'a ViewValue)],
) -> &'a ViewValue {
    match expression {
        Expression::Value(value) => value,
        Expression::Not(expression) => boolean(!truthy(value(expression, data, bindings))),
        Expression::Binary(operator, left, right) => {
            let left = value(left, data, bindings);
            let right = || value(right, data, bindings);
            match operator {
                BinaryOperator::Coalesce => {
                    if left.is_null() {
                        right()
                    } else {
                        left
                    }
                }
                BinaryOperator::Or => boolean(truthy(left) || truthy(right())),
                BinaryOperator::And => boolean(truthy(left) && truthy(right())),
                BinaryOperator::StrictEqual => boolean(strict_equal(left, right())),
                BinaryOperator::StrictNotEqual => boolean(!strict_equal(left, right())),
                BinaryOperator::Less => boolean(compare(left, right()) == Some(Ordering::Less)),
                BinaryOperator::LessOrEqual => boolean(matches!(
                    compare(left, right()),
                    Some(Ordering::Less | Ordering::Equal)
                )),
                BinaryOperator::Greater => {
                    boolean(compare(left, right()) == Some(Ordering::Greater))
                }
                BinaryOperator::GreaterOrEqual => boolean(matches!(
                    compare(left, right()),
                    Some(Ordering::Greater | Ordering::Equal)
                )),
            }
        }
        Expression::Variable(path) => {
            let Some((name, segments)) = path.split_first() else {
                return &ViewValue::Null;
            };
            let mut current = bindings
                .iter()
                .rev()
                .find_map(|(key, value)| (*key == name).then_some(*value))
                .or_else(|| data.get(name));
            for segment in segments {
                current = current.and_then(|value| value.get(segment));
            }
            current.unwrap_or(&ViewValue::Null)
        }
    }
}

fn compare(left: &ViewValue, right: &ViewValue) -> Option<Ordering> {
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return Some(left.cmp(right));
    }
    let integer = |value: &ViewValue| {
        value
            .as_i64()
            .map(i128::from)
            .or_else(|| value.as_u64().map(i128::from))
    };
    if let (Some(left), Some(right)) = (integer(left), integer(right)) {
        return Some(left.cmp(&right));
    }
    left.as_f64()?.partial_cmp(&right.as_f64()?)
}

pub(super) fn loose_equal(left: &ViewValue, right: &ViewValue) -> bool {
    use serde_json::Value::{Array, Bool, Null, Number, Object, String};
    match (left, right) {
        (Bool(_), _) | (_, Bool(_)) => truthy(left) == truthy(right),
        (Null, String(text)) | (String(text), Null) => text.is_empty(),
        (Null, _) | (_, Null) => truthy(left) == truthy(right),
        (String(left), String(right)) => {
            if let (Some(left), Some(right)) = (integer_text(left), integer_text(right)) {
                left == right
            } else {
                numeric_text(left)
                    .zip(numeric_text(right))
                    .map_or_else(|| left == right, |(left, right)| left == right)
            }
        }
        (Number(_), String(text)) | (String(text), Number(_)) => {
            let number = if left.is_number() { left } else { right };
            if let (Some(number), Some(text)) = (number.as_i64(), integer_text(text)) {
                number == text
            } else {
                number
                    .as_f64()
                    .zip(numeric_text(text))
                    .is_some_and(|(number, text)| number == text)
            }
        }
        (Number(_), Number(_)) => compare(left, right) == Some(Ordering::Equal),
        (Array(left), Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| loose_equal(left, right))
        }
        (Object(left), Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, value)| {
                    right
                        .get(key)
                        .is_some_and(|other| loose_equal(value, other))
                })
        }
        _ => false,
    }
}

fn integer_text(value: &str) -> Option<i64> {
    value
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .parse()
        .ok()
}

fn numeric_text(value: &str) -> Option<f64> {
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace());
    if value
        .bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'+' | b'-' | b'.' | b'e' | b'E'))
    {
        value.parse().ok()
    } else {
        None
    }
}

fn boolean(value: bool) -> &'static ViewValue {
    if value {
        &ViewValue::Bool(true)
    } else {
        &ViewValue::Bool(false)
    }
}

fn strict_equal(left: &ViewValue, right: &ViewValue) -> bool {
    match (left, right) {
        (ViewValue::Object(left), ViewValue::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|((key, value), (other_key, other))| {
                        key == other_key && strict_equal(value, other)
                    })
        }
        (ViewValue::Array(left), ViewValue::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(value, other)| strict_equal(value, other))
        }
        _ => left == right,
    }
}

pub(super) fn truthy(value: &ViewValue) -> bool {
    match value {
        ViewValue::Null => false,
        ViewValue::Bool(value) => *value,
        ViewValue::Number(value) => value.as_f64().is_some_and(|number| number != 0.0),
        ViewValue::String(value) => !value.is_empty() && value != "0",
        ViewValue::Array(value) => !value.is_empty(),
        ViewValue::Object(value) => !value.is_empty(),
    }
}

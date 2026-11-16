use std::{borrow::Cow, cmp::Ordering};

use serde::{Deserialize, Serialize};

use super::{
    Quote, ViewData, ViewResult, ViewValue,
    attributes::{array_entries, array_entry},
    identifier, invalid, separators,
};

#[derive(Serialize, Deserialize)]
pub(super) enum Expression {
    Value(ViewValue),
    Variable(Vec<String>),
    Not(Box<Expression>),
    Binary(BinaryOperator, Box<Expression>, Box<Expression>),
    Array(Vec<ArrayEntry>),
    Offset(Box<Expression>, Box<Expression>),
}

#[derive(Serialize, Deserialize)]
pub(super) struct ArrayEntry {
    key: Option<Expression>,
    item: Expression,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum BinaryOperator {
    Coalesce,
    Or,
    And,
    StrictEqual,
    StrictNotEqual,
    Equal,
    NotEqual,
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
    if !expression.starts_with('[')
        && let Ok(value) = serde_json::from_str(expression)
    {
        return Ok(Expression::Value(value));
    }
    for operators in [
        [("??", BinaryOperator::Coalesce)].as_slice(),
        &[("||", BinaryOperator::Or)],
        &[("&&", BinaryOperator::And)],
        &[
            ("===", BinaryOperator::StrictEqual),
            ("!==", BinaryOperator::StrictNotEqual),
            ("==", BinaryOperator::Equal),
            ("!=", BinaryOperator::NotEqual),
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
                        && (!matches!(*token, "==" | "!=")
                            || (!expression[..*index].ends_with(['=', '!'])
                                && !expression[*index + 2..].starts_with('=')))
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
    if let Some(start) = offset_start(expression) {
        let base = parse_expression(&expression[..start], depth + 1)?;
        let key = parse_expression(&expression[start + 1..expression.len() - 1], depth + 1)?;
        return Ok(Expression::Offset(Box::new(base), Box::new(key)));
    }
    if expression.starts_with('[') {
        return parse_array(array_entries(expression)?, depth);
    }
    if let Some(items) = expression
        .strip_prefix("array(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return parse_array(super::attributes::array_items(items), depth);
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

fn parse_array(entries: Vec<&str>, depth: usize) -> ViewResult<Expression> {
    entries
        .into_iter()
        .map(|entry| {
            let (first, second) = array_entry(entry);
            let (key, item) = match second {
                Some(item) => (Some(parse_expression(first, depth + 1)?), item),
                None => (None, first),
            };
            Ok(ArrayEntry {
                key,
                item: parse_expression(item, depth + 1)?,
            })
        })
        .collect::<ViewResult<_>>()
        .map(Expression::Array)
}

fn offset_start(source: &str) -> Option<usize> {
    if !source.ends_with(']') {
        return None;
    }
    let mut quote = Quote::default();
    let mut depth = 0usize;
    let mut start = None;
    for (position, character) in source.char_indices() {
        if quote.literal(character) {
            continue;
        }
        match character {
            '[' | '(' | '{' => {
                if character == '[' && depth == 0 {
                    start = Some(position);
                }
                depth += 1;
            }
            ']' | ')' | '}' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    start.filter(|start| *start > 0 && depth == 0)
}

fn integer_key(key: &str) -> Option<i64> {
    key.parse::<i64>()
        .ok()
        .filter(|value| value.to_string() == key)
}

fn array_key(value: &ViewValue) -> Option<String> {
    match value {
        ViewValue::String(value) => Some(value.clone()),
        ViewValue::Null => Some(String::new()),
        ViewValue::Bool(value) => Some(u8::from(*value).to_string()),
        ViewValue::Number(value) => value
            .as_i64()
            .or_else(|| {
                let float = value.as_f64()?;
                ((i64::MIN as f64..-(i64::MIN as f64)).contains(&float)).then_some(float as i64)
            })
            .map(|number| number.to_string()),
        _ => None,
    }
}

fn evaluate_array(
    entries: &[ArrayEntry],
    data: &ViewData,
    bindings: &[(&str, &ViewValue)],
) -> Option<ViewValue> {
    let mut output = ViewData::new();
    let mut highest = None::<i64>;
    for entry in entries {
        let key = match &entry.key {
            Some(key) => array_key(value(key, data, bindings).as_ref())?,
            None => highest
                .map_or(Some(0), |index| index.checked_add(1))?
                .to_string(),
        };
        if let Some(index) = integer_key(&key) {
            highest = Some(highest.map_or(index, |highest| highest.max(index)));
        }
        let item = value(&entry.item, data, bindings).into_owned();
        output.insert(key, item);
    }
    if output
        .keys()
        .enumerate()
        .all(|(index, key)| integer_key(key).is_some_and(|key| usize::try_from(key) == Ok(index)))
    {
        Some(ViewValue::Array(
            output.into_iter().map(|(_, value)| value).collect(),
        ))
    } else {
        Some(ViewValue::Object(output))
    }
}

fn element<'a>(array: &'a ViewValue, key: &str) -> &'a ViewValue {
    let found = match array {
        ViewValue::Object(array) => array.get(key),
        ViewValue::Array(array) => integer_key(key)
            .and_then(|key| usize::try_from(key).ok())
            .and_then(|key| array.get(key)),
        _ => None,
    };
    found.unwrap_or(&ViewValue::Null)
}

pub(super) fn value<'a>(
    expression: &'a Expression,
    data: &'a ViewData,
    bindings: &[(&str, &'a ViewValue)],
) -> Cow<'a, ViewValue> {
    match expression {
        Expression::Value(value) => Cow::Borrowed(value),
        Expression::Array(entries) => {
            Cow::Owned(evaluate_array(entries, data, bindings).unwrap_or_default())
        }
        Expression::Offset(base, key) => {
            let base = value(base, data, bindings);
            let Some(key) = array_key(value(key, data, bindings).as_ref()) else {
                return Cow::Borrowed(&ViewValue::Null);
            };
            match base {
                Cow::Borrowed(array) => Cow::Borrowed(element(array, &key)),
                Cow::Owned(array) => Cow::Owned(element(&array, &key).clone()),
            }
        }
        Expression::Not(expression) => {
            Cow::Borrowed(boolean(!truthy(value(expression, data, bindings).as_ref())))
        }
        Expression::Binary(operator, left, right) => {
            let left = value(left, data, bindings);
            let right = || value(right, data, bindings);
            if matches!(operator, BinaryOperator::Coalesce) {
                return if left.is_null() { right() } else { left };
            }
            let left = left.as_ref();
            let result = match operator {
                BinaryOperator::Coalesce => unreachable!(),
                BinaryOperator::Or => truthy(left) || truthy(right().as_ref()),
                BinaryOperator::And => truthy(left) && truthy(right().as_ref()),
                BinaryOperator::StrictEqual => strict_equal(left, right().as_ref()),
                BinaryOperator::StrictNotEqual => !strict_equal(left, right().as_ref()),
                BinaryOperator::Equal => loose_equal(left, right().as_ref()),
                BinaryOperator::NotEqual => !loose_equal(left, right().as_ref()),
                BinaryOperator::Less => compare(left, right().as_ref()) == Some(Ordering::Less),
                BinaryOperator::LessOrEqual => matches!(
                    compare(left, right().as_ref()),
                    Some(Ordering::Less | Ordering::Equal)
                ),
                BinaryOperator::Greater => {
                    compare(left, right().as_ref()) == Some(Ordering::Greater)
                }
                BinaryOperator::GreaterOrEqual => matches!(
                    compare(left, right().as_ref()),
                    Some(Ordering::Greater | Ordering::Equal)
                ),
            };
            Cow::Borrowed(boolean(result))
        }
        Expression::Variable(path) => {
            let Some((name, segments)) = path.split_first() else {
                return Cow::Borrowed(&ViewValue::Null);
            };
            let mut current = bindings
                .iter()
                .rev()
                .find_map(|(key, value)| (*key == name).then_some(*value))
                .or_else(|| data.get(name));
            for segment in segments {
                current = current.and_then(|value| value.get(segment));
            }
            Cow::Borrowed(current.unwrap_or(&ViewValue::Null))
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

pub(super) fn numeric_text(value: &str) -> Option<f64> {
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

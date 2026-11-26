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
    Translation(Box<super::translations::Translation>),
    Helper(Box<super::helpers::Helper>),
    Application(Vec<Expression>),
    Method(Box<Expression>, String, Vec<Expression>),
    Variable(Vec<String>),
    Not(Box<Expression>),
    Numeric(super::arithmetic::Literal),
    Sign(bool, Box<Expression>),
    Arithmetic(
        super::arithmetic::Operator,
        Box<Expression>,
        Box<Expression>,
    ),
    Binary(BinaryOperator, Box<Expression>, Box<Expression>),
    Array(Vec<ArrayEntry>),
    Offset(Box<Expression>, Box<Expression>),
}

#[derive(Serialize, Deserialize)]
pub(super) enum ArrayEntry {
    Item {
        key: Option<Expression>,
        item: Expression,
    },
    Unpack(Expression),
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

fn parse_arguments(source: &str, depth: usize) -> ViewResult<Vec<Expression>> {
    if source.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut start = 0;
    separators(source, ",")
        .chain(std::iter::once(source.len()))
        .map(|end| {
            let argument = parse_expression(&source[start..end], depth + 1);
            start = end + 1;
            argument
        })
        .collect()
}

pub(super) fn expression_value(expression: &str) -> ViewResult<Expression> {
    parse_expression(expression, 0)
}

pub(super) fn parse_expression(expression: &str, depth: usize) -> ViewResult<Expression> {
    if depth > 48 {
        return Err(invalid("Blade expressions are nested too deeply."));
    }
    let expression = expression.trim();
    if let Some(number) = super::arithmetic::Number::literal(expression) {
        return Ok(Expression::Numeric(number.literal_node()));
    }
    if !expression.starts_with('[')
        && let Ok(value) = serde_json::from_str::<ViewValue>(expression)
        && !value.is_number()
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
    if super::separators(expression, "++").next().is_some()
        || super::separators(expression, "--").next().is_some()
    {
        return Err(invalid("Unsupported Blade mutation expression."));
    }
    use super::arithmetic::{Operator, binary_position};
    for operators in [
        [("+", Operator::Add), ("-", Operator::Subtract)].as_slice(),
        &[
            ("*", Operator::Multiply),
            ("/", Operator::Divide),
            ("%", Operator::Modulo),
        ],
    ] {
        if let Some((index, token, operator)) = operators
            .iter()
            .filter_map(|(token, operator)| {
                binary_position(expression, token, false).map(|i| (i, *token, *operator))
            })
            .max_by_key(|(i, _, _)| *i)
        {
            return Ok(Expression::Arithmetic(
                operator,
                Box::new(parse_expression(&expression[..index], depth + 1)?),
                Box::new(parse_expression(
                    &expression[index + token.len()..],
                    depth + 1,
                )?),
            ));
        }
    }
    if expression.starts_with("++") || expression.starts_with("--") {
        return Err(invalid("Unsupported Blade mutation expression."));
    }
    if let Some(rest) = expression
        .strip_prefix('+')
        .or_else(|| expression.strip_prefix('-'))
    {
        return Ok(Expression::Sign(
            expression.starts_with('-'),
            Box::new(parse_expression(rest, depth + 1)?),
        ));
    }
    if let Some(rest) = expression.strip_prefix('!') {
        return Ok(Expression::Not(Box::new(parse_expression(
            rest,
            depth + 1,
        )?)));
    }
    if let Some(index) = binary_position(expression, "**", true) {
        return Ok(Expression::Arithmetic(
            Operator::Power,
            Box::new(parse_expression(&expression[..index], depth + 1)?),
            Box::new(parse_expression(&expression[index + 2..], depth + 1)?),
        ));
    }
    if let Some(group) = parenthesized(expression) {
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
    // 成员调用按通用语法解析；具体方法由接收对象负责，不在解析器维护服务方法清单。
    if !expression.starts_with("request()")
        && let Some(index) = separators(expression, "->").last()
    {
        let call = &expression[index + 2..];
        if let Some(open) = call.find('(')
            && let Some(arguments) = call[open + 1..].strip_suffix(')')
            && identifier(call[..open].trim())
        {
            return Ok(Expression::Method(
                Box::new(parse_expression(&expression[..index], depth + 1)?),
                call[..open].trim().to_owned(),
                parse_arguments(arguments, depth)?,
            ));
        }
    }
    if let Some(arguments) = expression
        .strip_prefix("app(")
        .and_then(|v| v.strip_suffix(')'))
    {
        let arguments = parse_arguments(arguments, depth)?;
        if arguments.len() > 2 {
            return Err(invalid("app 最多接受服务名与构造参数两个参数。"));
        }
        return Ok(Expression::Application(arguments));
    }
    if let Some(translation) = super::translations::Translation::parse(expression, depth)? {
        return Ok(Expression::Translation(Box::new(translation)));
    }
    if let Some(helper) = super::helpers::Helper::parse(expression, depth)? {
        return Ok(Expression::Helper(Box::new(helper)));
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

// 仅去掉包住整个表达式的括号，保留 (object)->method() 的接收对象边界。
fn parenthesized(expression: &str) -> Option<&str> {
    if !expression.starts_with('(') {
        return None;
    }
    let mut quote = Quote::default();
    let mut depth = 0usize;
    for (index, character) in expression.char_indices() {
        if quote.literal(character) {
            continue;
        }
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return (index + 1 == expression.len()).then_some(&expression[1..index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_array(entries: Vec<&str>, depth: usize) -> ViewResult<Expression> {
    entries
        .into_iter()
        .map(|entry| {
            if let Some(source) = entry.strip_prefix("...") {
                return Ok(ArrayEntry::Unpack(parse_expression(source, depth + 1)?));
            }
            let (first, second) = array_entry(entry);
            let (key, item) = match second {
                Some(item) => (Some(parse_expression(first, depth + 1)?), item),
                None => (None, first),
            };
            Ok(ArrayEntry::Item {
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
        ViewValue::Number(value) => {
            super::arithmetic::Number::from_value(&ViewValue::Number(value.clone()))
                .ok()
                .map(|number| number.modulo_integer().to_string())
        }
        _ => None,
    }
}

fn evaluate_array(
    entries: &[ArrayEntry],
    data: &ViewData,
    bindings: &[(&str, &ViewValue)],
) -> ViewResult<ViewValue> {
    let mut output = ViewData::new();
    let mut highest = None::<i64>;
    for entry in entries {
        match entry {
            ArrayEntry::Unpack(expression) => {
                let source = value(expression, data, bindings)?;
                match source.as_ref() {
                    ViewValue::Array(values) => {
                        for item in values {
                            let key = next_array_index(highest)?;
                            highest = integer_key(&key);
                            output.insert(key, item.clone());
                        }
                    }
                    ViewValue::Object(values) => {
                        for (key, item) in values {
                            let key = if integer_key(key).is_some() {
                                let key = next_array_index(highest)?;
                                highest = integer_key(&key);
                                key
                            } else {
                                key.clone()
                            };
                            output.insert(key, item.clone());
                        }
                    }
                    _ => {
                        return Err(render_error(
                            "Only arrays can be unpacked in Blade array expressions.",
                        ));
                    }
                }
            }
            ArrayEntry::Item { key, item } => {
                let key = match key {
                    Some(key) => array_key(value(key, data, bindings)?.as_ref())
                        .ok_or_else(|| render_error("Illegal Blade array key type."))?,
                    None => next_array_index(highest)?,
                };
                if let Some(index) = integer_key(&key) {
                    highest = Some(highest.map_or(index, |highest| highest.max(index)));
                }
                let item = value(item, data, bindings)?.into_owned();
                output.insert(key, item);
            }
        }
    }
    if output
        .keys()
        .enumerate()
        .all(|(index, key)| integer_key(key).is_some_and(|key| usize::try_from(key) == Ok(index)))
    {
        Ok(ViewValue::Array(
            output.into_iter().map(|(_, value)| value).collect(),
        ))
    } else {
        Ok(ViewValue::Object(output))
    }
}

fn next_array_index(highest: Option<i64>) -> ViewResult<String> {
    highest
        .map_or(Some(0), |index| index.checked_add(1))
        .map(|index| index.to_string())
        .ok_or_else(|| render_error("Blade array index overflow."))
}

fn element<'a>(array: &'a ViewValue, key: &str) -> Option<&'a ViewValue> {
    match array {
        ViewValue::Object(array) => array.get(key),
        ViewValue::Array(array) => integer_key(key)
            .and_then(|key| usize::try_from(key).ok())
            .and_then(|key| array.get(key)),
        _ => None,
    }
}

use super::arithmetic::Number;
use crate::view::rendering::render_error;

pub(super) enum Evaluated<'a> {
    Json(Cow<'a, ViewValue>),
    Number(Number),
}
impl<'a> Evaluated<'a> {
    pub(super) fn json(self) -> ViewResult<Cow<'a, ViewValue>> {
        match self {
            Self::Json(v) => Ok(v),
            Self::Number(n) => Ok(Cow::Owned(n.json()?)),
        }
    }
    fn number(&self) -> ViewResult<Number> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::Json(v) => Number::from_value(v),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Self::Json(v) => truthy(v),
            Self::Number(n) => n.double() != 0.0,
        }
    }
    fn is_null(&self) -> bool {
        matches!(self,Self::Json(v) if v.is_null())
    }
    pub(super) fn numeric_text(&self) -> Option<String> {
        match self {
            Self::Number(Number::Integer(v)) => Some(v.to_string()),
            Self::Number(Number::Float(v)) => {
                Some(crate::support::input_scalars::double_string(*v))
            }
            _ => None,
        }
    }
}
pub(super) fn value<'a>(
    expression: &'a Expression,
    data: &'a ViewData,
    bindings: &[(&str, &'a ViewValue)],
) -> ViewResult<Cow<'a, ViewValue>> {
    evaluate(expression, data, bindings)?.json()
}
pub(super) fn boolean_value(
    expression: &Expression,
    data: &ViewData,
    bindings: &[(&str, &ViewValue)],
) -> ViewResult<bool> {
    Ok(evaluate(expression, data, bindings)?.truthy())
}
pub(super) fn condition_value<'a>(
    expression: &'a Expression,
    data: &'a ViewData,
    bindings: &[(&str, &'a ViewValue)],
    soft: bool,
) -> ViewResult<Cow<'a, ViewValue>> {
    let result = evaluate_with(expression, data, bindings, soft)?;
    if let Evaluated::Number(n) = &result
        && !n.double().is_finite()
    {
        return Ok(Cow::Borrowed(boolean(true)));
    }
    result.json()
}
pub(super) fn evaluate<'a>(
    expression: &'a Expression,
    data: &'a ViewData,
    bindings: &[(&str, &'a ViewValue)],
) -> ViewResult<Evaluated<'a>> {
    evaluate_with(expression, data, bindings, false)
}
fn evaluate_with<'a>(
    expression: &'a Expression,
    data: &'a ViewData,
    bindings: &[(&str, &'a ViewValue)],
    soft: bool,
) -> ViewResult<Evaluated<'a>> {
    Ok(match expression {
        Expression::Numeric(number) => Evaluated::Number(number.number()),
        Expression::Sign(negative, expression) => {
            let number = evaluate(expression, data, bindings)?.number()?;
            Evaluated::Number(if *negative { number.negate() } else { number })
        }
        Expression::Arithmetic(operator, left, right) => {
            let left = evaluate(left, data, bindings)?;
            let right = evaluate(right, data, bindings)?;
            if matches!(operator, super::arithmetic::Operator::Modulo) {
                let integer = |value: &Evaluated<'_>| match value {
                    Evaluated::Number(n) => Ok(n.modulo_integer()),
                    Evaluated::Json(v) => Number::modulo_value(v),
                };
                Evaluated::Number(
                    Number::Integer(integer(&left)?)
                        .operation(*operator, Number::Integer(integer(&right)?))?,
                )
            } else if matches!(operator, super::arithmetic::Operator::Add)
                && matches!((&left,&right),(Evaluated::Json(a),Evaluated::Json(b)) if (a.is_array()||a.is_object()) && (b.is_array()||b.is_object()))
            {
                let mut entries = ViewData::new();
                for source in [left.json()?, right.json()?] {
                    match source.as_ref() {
                        ViewValue::Object(values) => {
                            for (k, v) in values {
                                entries.entry(k.clone()).or_insert_with(|| v.clone());
                            }
                        }
                        ViewValue::Array(values) => {
                            for (i, v) in values.iter().enumerate() {
                                entries.entry(i.to_string()).or_insert_with(|| v.clone());
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                let array = entries.keys().enumerate().all(|(i, k)| k == &i.to_string());
                Evaluated::Json(Cow::Owned(if array {
                    ViewValue::Array(entries.into_values().collect())
                } else {
                    ViewValue::Object(entries)
                }))
            } else {
                Evaluated::Number(left.number()?.operation(*operator, right.number()?)?)
            }
        }
        Expression::Translation(translation) => {
            Evaluated::Json(Cow::Owned(translation.evaluate(data, bindings)?))
        }
        Expression::Helper(helper) => Evaluated::Json(Cow::Owned(helper.evaluate(data, bindings)?)),
        Expression::Application(arguments) => {
            let arguments = arguments
                .iter()
                .map(|argument| value(argument, data, bindings).map(Cow::into_owned))
                .collect::<ViewResult<Vec<_>>>()?;
            crate::view::object::validate_arguments(&arguments)?;
            let application = crate::view::object::application()?;
            let value = if arguments.is_empty() || arguments[0].is_null() {
                application
            } else {
                let ViewValue::Instance(object) = application else {
                    unreachable!()
                };
                crate::view::request_data::call_object(&object, "make", arguments)?
            };
            Evaluated::Json(Cow::Owned(value))
        }
        Expression::Method(receiver, method, arguments) => {
            let receiver = value(receiver, data, bindings)?;
            let ViewValue::Instance(object) = receiver.as_ref() else {
                return Err(render_error("Blade 方法调用需要服务对象。"));
            };
            let arguments = arguments
                .iter()
                .map(|argument| value(argument, data, bindings).map(Cow::into_owned))
                .collect::<ViewResult<Vec<_>>>()?;
            Evaluated::Json(Cow::Owned(crate::view::request_data::call_object(
                object, method, arguments,
            )?))
        }

        Expression::Value(v) => Evaluated::Json(Cow::Borrowed(v)),
        Expression::Array(entries) => {
            Evaluated::Json(Cow::Owned(evaluate_array(entries, data, bindings)?))
        }
        Expression::Offset(base, key) => {
            let base = evaluate_with(base, data, bindings, soft)?.json()?;
            let key = array_key(value(key, data, bindings)?.as_ref())
                .ok_or_else(|| render_error("Illegal Blade array key type."))?;
            let found = element(&base, &key);
            if found.is_none() && !soft {
                return Err(render_error("Undefined Blade array key."));
            }
            Evaluated::Json(match base {
                Cow::Borrowed(a) => Cow::Borrowed(element(a, &key).unwrap_or(&ViewValue::Null)),
                Cow::Owned(a) => Cow::Owned(element(&a, &key).cloned().unwrap_or_default()),
            })
        }
        Expression::Not(expression) => Evaluated::Json(Cow::Borrowed(boolean(
            !evaluate(expression, data, bindings)?.truthy(),
        ))),
        Expression::Binary(operator, left, right) => {
            let left = evaluate_with(
                left,
                data,
                bindings,
                matches!(operator, BinaryOperator::Coalesce),
            )?;
            let right = || evaluate(right, data, bindings);
            if matches!(operator, BinaryOperator::Coalesce) {
                return if left.is_null() { right() } else { Ok(left) };
            }
            let result = match operator {
                BinaryOperator::Or => left.truthy() || right()?.truthy(),
                BinaryOperator::And => left.truthy() && right()?.truthy(),
                _ => compare_evaluated(*operator, left, right()?)?,
            };
            Evaluated::Json(Cow::Borrowed(boolean(result)))
        }
        Expression::Variable(path) => {
            let Some((name, segments)) = path.split_first() else {
                return Ok(Evaluated::Json(Cow::Borrowed(&ViewValue::Null)));
            };
            let mut current = bindings
                .iter()
                .rev()
                .find_map(|(key, value)| (*key == name).then_some(*value))
                .or_else(|| data.get(name))
                .map(Cow::Borrowed);
            for segment in segments {
                current = match current {
                    Some(value) if matches!(value.as_ref(), ViewValue::Instance(_)) => {
                        let ViewValue::Instance(object) = value.as_ref() else {
                            unreachable!()
                        };
                        object.property(segment)?.map(Cow::Owned)
                    }
                    Some(Cow::Borrowed(value)) => value.get(segment).map(Cow::Borrowed),
                    Some(Cow::Owned(value)) => value.get(segment).cloned().map(Cow::Owned),
                    None => None,
                };
            }
            if current.is_none() && !soft {
                return Err(render_error("Undefined Blade variable or property."));
            }
            Evaluated::Json(current.unwrap_or(Cow::Borrowed(&ViewValue::Null)))
        }
    })
}
fn compare_evaluated(
    operator: BinaryOperator,
    left: Evaluated<'_>,
    right: Evaluated<'_>,
) -> ViewResult<bool> {
    let special = |v: &Evaluated<'_>| match v {
        Evaluated::Number(n) => !n.double().is_finite(),
        Evaluated::Json(v) => matches!(v.as_ref(), ViewValue::Number(n) if !n.is_finite()),
    };
    let (equal, order, strict) = if special(&left) || special(&right) {
        let (number, other, reverse) = if special(&left) {
            (&left, &right, false)
        } else {
            (&right, &left, true)
        };
        let n = number.number()?.double();
        let (order, strict) = match other {
            Evaluated::Number(m) => (n.partial_cmp(&m.double()), matches!(m, Number::Float(_))),
            Evaluated::Json(v) => match v.as_ref() {
                ViewValue::Bool(_) | ViewValue::Null => {
                    (Some(number.truthy().cmp(&other.truthy())), false)
                }
                ViewValue::Number(m) => {
                    (n.partial_cmp(&m.as_f64().expect("JSON number")), m.is_f64())
                }
                ViewValue::String(_) if n.is_nan() => (None, false),
                ViewValue::String(s) => (
                    numeric_text(s).map_or_else(
                        || Some(crate::support::input_scalars::double_string(n).cmp(s)),
                        |v| n.partial_cmp(&v),
                    ),
                    false,
                ),
                _ => (Some(Ordering::Less), false),
            },
        };
        let equal = order == Some(Ordering::Equal);
        (
            equal,
            if reverse {
                order.map(Ordering::reverse)
            } else {
                order
            },
            strict && equal,
        )
    } else {
        let (left, right) = (left.json()?, right.json()?);
        (
            loose_equal(&left, &right),
            compare(&left, &right),
            strict_equal(&left, &right),
        )
    };
    Ok(match operator {
        BinaryOperator::StrictEqual => strict,
        BinaryOperator::StrictNotEqual => !strict,
        BinaryOperator::Equal => equal,
        BinaryOperator::NotEqual => !equal,
        BinaryOperator::Less => order == Some(Ordering::Less),
        BinaryOperator::LessOrEqual => matches!(order, Some(Ordering::Less | Ordering::Equal)),
        BinaryOperator::Greater => order == Some(Ordering::Greater),
        BinaryOperator::GreaterOrEqual => {
            matches!(order, Some(Ordering::Greater | Ordering::Equal))
        }
        _ => unreachable!(),
    })
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
    use ViewValue::{Array, Bool, Null, Number, Object, String};
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
            if let Some(number) = number.as_f64().filter(|number| !number.is_finite()) {
                !number.is_nan()
                    && numeric_text(text).map_or_else(
                        || crate::support::input_scalars::double_string(number) == *text,
                        |other| number == other,
                    )
            } else if let (Some(number), Some(text)) = (number.as_i64(), integer_text(text)) {
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
        (ViewValue::Instance(left), ViewValue::Instance(right)) => left == right,
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

pub(super) fn strict_equal(left: &ViewValue, right: &ViewValue) -> bool {
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
        ViewValue::Instance(_) => true,
    }
}

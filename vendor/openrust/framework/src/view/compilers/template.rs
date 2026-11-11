use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::view::{ViewData, ViewError, ViewResult, ViewValue};

#[derive(Serialize, Deserialize)]
pub(super) struct Template(Vec<Node>);

#[derive(Serialize, Deserialize)]
enum Node {
    Text(String),
    Echo(Expression, bool),
    Conditional(Expression, Predicate, Vec<Node>, Vec<Node>),
    Each(Expression, String, Vec<Node>, Vec<Node>),
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum Predicate {
    Truthy,
    Empty,
    Isset,
}

#[derive(Serialize, Deserialize)]
enum Expression {
    Value(ViewValue),
    Variable(Vec<String>),
}

impl Template {
    pub(super) fn compile(source: &str) -> ViewResult<Self> {
        let mut parser = Parser(source);
        let (nodes, stop) = parser.block(0)?;
        if stop.is_some() {
            return Err(invalid("Unexpected closing Blade directive."));
        }
        Ok(Self(nodes))
    }

    pub(super) fn render(&self, data: &ViewData) -> String {
        let mut output = String::new();
        render_nodes(&self.0, data, &mut Vec::new(), &mut output);
        output
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Stop {
    Else,
    ElseIf,
    EndIf,
    EndUnless,
    EndForeach,
    EndForelse,
    Empty,
    EndEmpty,
    EndIsset,
}

struct Parser<'a>(&'a str);

impl<'a> Parser<'a> {
    fn block(&mut self, depth: usize) -> ViewResult<(Vec<Node>, Option<Stop>)> {
        if depth > 64 {
            return Err(invalid("Blade blocks are nested too deeply."));
        }
        let mut nodes = Vec::new();
        let mut text = String::new();
        while !self.0.is_empty() {
            let index = self.0.find(['@', '{']).unwrap_or(self.0.len());
            text.push_str(&self.0[..index]);
            self.0 = &self.0[index..];
            if self.0.is_empty() {
                break;
            }
            if self.0.starts_with("@{{") || self.0.starts_with("@{!!") {
                self.0 = &self.0[1..];
                let end = if self.0.starts_with("{{") {
                    "}}"
                } else {
                    "!!}"
                };
                let prefix = if end == "}}" { 2 } else { 3 };
                let literal = self.delimited(prefix, end)?;
                write!(
                    text,
                    "{}{literal}{end}",
                    if end == "}}" { "{{" } else { "{!!" }
                )
                .unwrap();
            } else if self.0.starts_with("@@") {
                text.push('@');
                self.0 = &self.0[2..];
            } else if self.0.starts_with("{{--") {
                self.delimited(4, "--}}")?;
            } else if self.0.starts_with("{{") || self.0.starts_with("{!!") {
                push_text(&mut nodes, &mut text);
                let escaped = self.0.starts_with("{{");
                let expression = if escaped {
                    self.delimited(2, "}}")?
                } else {
                    self.delimited(3, "!!}")?
                };
                nodes.push(Node::Echo(expression_value(expression)?, escaped));
            } else if self.directive("verbatim") {
                text.push_str(self.delimited(0, "@endverbatim")?);
            } else if let Some(stop) = self.stop() {
                push_text(&mut nodes, &mut text);
                return Ok((nodes, Some(stop)));
            } else if let Some((predicate, end)) = self.condition() {
                push_text(&mut nodes, &mut text);
                nodes.push(self.conditional(depth, predicate, end)?);
            } else if let Some(foreach) = self.iteration() {
                push_text(&mut nodes, &mut text);
                nodes.push(self.loop_body(depth, foreach)?);
            } else {
                let character = self.0.chars().next().unwrap();
                text.push(character);
                self.0 = &self.0[character.len_utf8()..];
            }
        }
        push_text(&mut nodes, &mut text);
        Ok((nodes, None))
    }

    fn conditional(&mut self, depth: usize, predicate: Predicate, end: Stop) -> ViewResult<Node> {
        let expression = expression_value(self.arguments()?)?;
        let (yes, stop) = self.block(depth + 1)?;
        let (no, stop) = match stop {
            Some(Stop::Else) => self.block(depth + 1)?,
            Some(Stop::ElseIf) if end == Stop::EndIf && depth < 64 => (
                vec![self.conditional(depth + 1, Predicate::Truthy, end)?],
                Some(end),
            ),
            _ => (Vec::new(), stop),
        };
        if stop != Some(end) {
            return Err(invalid("Unclosed Blade conditional."));
        }
        Ok(Node::Conditional(expression, predicate, yes, no))
    }

    fn condition(&mut self) -> Option<(Predicate, Stop)> {
        [
            ("if", Predicate::Truthy, Stop::EndIf),
            ("unless", Predicate::Empty, Stop::EndUnless),
            ("isset", Predicate::Isset, Stop::EndIsset),
            ("empty", Predicate::Empty, Stop::EndEmpty),
        ]
        .into_iter()
        .find_map(|(name, predicate, stop)| self.directive(name).then_some((predicate, stop)))
    }

    fn iteration(&mut self) -> Option<bool> {
        if self.directive("foreach") {
            return Some(true);
        }
        self.directive("forelse").then_some(false)
    }

    fn loop_body(&mut self, depth: usize, foreach: bool) -> ViewResult<Node> {
        let arguments = self.arguments()?.trim();
        let (binding, name) = arguments
            .rsplit_once(char::is_whitespace)
            .ok_or_else(|| invalid("Expected a Blade loop of the form $items as $item."))?;
        let variable = name
            .strip_prefix('$')
            .filter(|name| identifier(name))
            .ok_or_else(|| invalid("Invalid Blade loop variable."))?;
        let (expression, _) = binding
            .trim_end()
            .rsplit_once(char::is_whitespace)
            .filter(|(_, keyword)| *keyword == "as")
            .ok_or_else(|| invalid("Expected a Blade loop of the form $items as $item."))?;
        let expression = expression_value(expression)?;
        let (body, stop) = self.block(depth + 1)?;
        let alternative = match (foreach, stop) {
            (true, Some(Stop::EndForeach)) => Vec::new(),
            (false, Some(Stop::Empty)) => {
                let (alternative, stop) = self.block(depth + 1)?;
                if stop != Some(Stop::EndForelse) {
                    return Err(invalid("Expected @endforelse."));
                }
                alternative
            }
            _ => return Err(invalid("Unclosed Blade loop.")),
        };
        Ok(Node::Each(
            expression,
            variable.to_owned(),
            body,
            alternative,
        ))
    }

    fn stop(&mut self) -> Option<Stop> {
        if let Some(rest) = self.0.strip_prefix("@empty")
            && !rest.trim_start().starts_with('(')
            && self.directive("empty")
        {
            return Some(Stop::Empty);
        }
        [
            ("endif", Stop::EndIf),
            ("endunless", Stop::EndUnless),
            ("endforeach", Stop::EndForeach),
            ("endforelse", Stop::EndForelse),
            ("endisset", Stop::EndIsset),
            ("endempty", Stop::EndEmpty),
            ("elseif", Stop::ElseIf),
            ("else", Stop::Else),
        ]
        .into_iter()
        .find_map(|(name, stop)| self.directive(name).then_some(stop))
    }

    fn directive(&mut self, name: &str) -> bool {
        let Some(rest) = self
            .0
            .strip_prefix('@')
            .and_then(|rest| rest.strip_prefix(name))
        else {
            return false;
        };
        if rest.starts_with(|character: char| character.is_alphanumeric() || character == '_') {
            return false;
        }
        self.0 = rest;
        true
    }

    fn arguments(&mut self) -> ViewResult<&'a str> {
        self.0 = self.0.trim_start();
        if !self.0.starts_with('(') {
            return Err(invalid("Blade directive requires parentheses."));
        }
        self.delimited(1, ")")
    }

    fn delimited(&mut self, prefix: usize, delimiter: &str) -> ViewResult<&'a str> {
        let (value, rest) = self.0[prefix..]
            .split_once(delimiter)
            .ok_or_else(|| invalid("Unclosed Blade expression."))?;
        self.0 = rest;
        Ok(value)
    }
}

fn push_text(nodes: &mut Vec<Node>, text: &mut String) {
    if !text.is_empty() {
        nodes.push(Node::Text(std::mem::take(text)));
    }
}

fn expression_value(expression: &str) -> ViewResult<Expression> {
    let expression = expression.trim();
    if let Ok(value) = serde_json::from_str(expression) {
        return Ok(Expression::Value(value));
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

fn identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && characters.all(|character| character.is_alphanumeric() || character == '_')
}

fn value<'a>(
    expression: &'a Expression,
    data: &'a ViewData,
    bindings: &[(&str, &'a ViewValue)],
) -> &'a ViewValue {
    match expression {
        Expression::Value(value) => value,
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

fn render_nodes<'a>(
    nodes: &'a [Node],
    data: &'a ViewData,
    bindings: &mut Vec<(&'a str, &'a ViewValue)>,
    output: &mut String,
) {
    for node in nodes {
        match node {
            Node::Text(text) => output.push_str(text),
            Node::Echo(expression, escape) => {
                let value = value(expression, data, bindings);
                match value {
                    ViewValue::String(string) if *escape => escape_html(output, string),
                    ViewValue::String(string) => output.push_str(string),
                    ViewValue::Bool(true) => output.push('1'),
                    ViewValue::Number(number) => write!(output, "{number}").unwrap(),
                    _ => {}
                }
            }
            Node::Conditional(expression, predicate, yes, no) => {
                let value = value(expression, data, bindings);
                let matches = match predicate {
                    Predicate::Truthy => truthy(value),
                    Predicate::Empty => !truthy(value),
                    Predicate::Isset => !value.is_null(),
                };
                render_nodes(if matches { yes } else { no }, data, bindings, output);
            }
            Node::Each(expression, variable, body, alternative) => {
                let value = value(expression, data, bindings);
                let arrays = value.as_array().into_iter().flatten();
                let objects = value.as_object().into_iter().flat_map(|map| map.values());
                let mut values = arrays.chain(objects).peekable();
                if values.peek().is_none() {
                    render_nodes(alternative, data, bindings, output);
                }
                for value in values {
                    bindings.push((variable, value));
                    render_nodes(body, data, bindings, output);
                    bindings.pop();
                }
            }
        }
    }
}

fn truthy(value: &ViewValue) -> bool {
    match value {
        ViewValue::Null => false,
        ViewValue::Bool(value) => *value,
        ViewValue::Number(value) => value.as_f64().is_some_and(|number| number != 0.0),
        ViewValue::String(value) => !value.is_empty() && value != "0",
        ViewValue::Array(value) => !value.is_empty(),
        ViewValue::Object(value) => !value.is_empty(),
    }
}

fn escape_html(output: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#039;"),
            other => output.push(other),
        }
    }
}

fn invalid(message: &'static str) -> ViewError {
    ViewError::CompileFailed(std::io::Error::new(std::io::ErrorKind::InvalidData, message).into())
}

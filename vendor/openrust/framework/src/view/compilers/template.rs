use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::view::{
    ViewData, ViewError, ViewResult, ViewValue,
    rendering::{RenderContext, render_error},
};

#[derive(Serialize, Deserialize)]
pub(super) struct Template(Vec<Node>);

#[derive(Serialize, Deserialize)]
enum Node {
    Text(String),
    Echo(Expression, bool),
    Conditional(Expression, Predicate, Vec<Node>, Vec<Node>),
    Each(Expression, String, Vec<Node>, Vec<Node>),
    Extends(Expression),
    Parent,
    Section(Expression, Vec<Node>, bool),
    Yield(Expression, Option<Expression>),
    Include(Expression),
    Component {
        name: String,
        attributes: Vec<(String, Expression)>,
        body: Vec<Node>,
        slots: Vec<(String, Vec<Node>)>,
    },
    Slot(String, Vec<Node>),
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

    pub(super) fn render(
        &self,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let mut output = String::new();
        render_nodes(&self.0, data, &mut Vec::new(), &mut output, context)?;
        output.push_str(&context.render_layout(data)?);
        if output.contains(context.parent()) {
            output = output.replace(context.parent(), "");
        }
        Ok(output)
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
    EndSection,
    Show,
    EndTag,
}

struct Parser<'a>(&'a str);

impl<'a> Parser<'a> {
    fn block(&mut self, depth: usize) -> ViewResult<(Vec<Node>, Option<Stop>)> {
        self.block_until(depth, None)
    }

    fn block_until(
        &mut self,
        depth: usize,
        closing: Option<&str>,
    ) -> ViewResult<(Vec<Node>, Option<Stop>)> {
        if depth > 64 {
            return Err(invalid("Blade blocks are nested too deeply."));
        }
        let mut nodes = Vec::new();
        let mut text = String::new();
        while !self.0.is_empty() {
            let index = self.0.find(['@', '{', '<']).unwrap_or(self.0.len());
            text.push_str(&self.0[..index]);
            self.0 = &self.0[index..];
            if self.0.is_empty() {
                break;
            }
            if let Some(rest) = self.0.strip_prefix("</x-") {
                let (name, tail) = rest
                    .split_once('>')
                    .ok_or_else(|| invalid("Unclosed Blade component closing tag."))?;
                let name = name.trim_end();
                if closing != Some(name)
                    && !(name == "slot" && closing.is_some_and(|tag| tag.starts_with("slot:")))
                {
                    return Err(invalid("Mismatched Blade component closing tag."));
                }
                self.0 = tail;
                push_text(&mut nodes, &mut text);
                return Ok((nodes, Some(Stop::EndTag)));
            } else if self.0.starts_with("<x-") {
                push_text(&mut nodes, &mut text);
                nodes.push(self.component(depth)?);
            } else if self.0.starts_with("@{{") || self.0.starts_with("@{!!") {
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
            } else if let Some(node) = self.view_directive(depth)? {
                push_text(&mut nodes, &mut text);
                nodes.push(node);
            } else {
                let character = self.0.chars().next().unwrap();
                text.push(character);
                self.0 = &self.0[character.len_utf8()..];
            }
        }
        push_text(&mut nodes, &mut text);
        Ok((nodes, None))
    }

    fn component(&mut self, depth: usize) -> ViewResult<Node> {
        self.0 = &self.0[3..];
        let end = self
            .0
            .find(|character: char| character.is_whitespace() || matches!(character, '>' | '/'))
            .ok_or_else(|| invalid("Unclosed Blade component tag."))?;
        let name = &self.0[..end];
        self.0 = &self.0[end..];
        let (attributes, self_closing) = self.component_attributes()?;

        if let Some(slot) = name.strip_prefix("slot:") {
            if !identifier(slot) || !attributes.is_empty() || self_closing {
                return Err(invalid("Invalid Blade named slot."));
            }
            return Ok(Node::Slot(
                slot.to_owned(),
                self.component_body(depth, name)?,
            ));
        }
        if !name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        }) {
            return Err(invalid("Invalid Blade component name."));
        }
        let children = if self_closing {
            Vec::new()
        } else {
            self.component_body(depth, name)?
        };
        let mut body = Vec::new();
        let mut slots = Vec::new();
        for node in children {
            match node {
                Node::Slot(name, nodes) => slots.push((name, nodes)),
                node => body.push(node),
            }
        }
        Ok(Node::Component {
            name: name.to_owned(),
            attributes,
            body,
            slots,
        })
    }

    fn component_body(&mut self, depth: usize, name: &str) -> ViewResult<Vec<Node>> {
        let (body, stop) = self.block_until(depth + 1, Some(name))?;
        if stop != Some(Stop::EndTag) {
            return Err(invalid("Unclosed Blade component body."));
        }
        Ok(body)
    }

    fn component_attributes(&mut self) -> ViewResult<(Vec<(String, Expression)>, bool)> {
        let mut attributes = Vec::new();
        loop {
            self.0 = self.0.trim_start();
            if let Some(rest) = self.0.strip_prefix("/>") {
                self.0 = rest;
                return Ok((attributes, true));
            }
            if let Some(rest) = self.0.strip_prefix('>') {
                self.0 = rest;
                return Ok((attributes, false));
            }
            let end = self
                .0
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, '=' | '>' | '/')
                })
                .ok_or_else(|| invalid("Unclosed Blade component tag."))?;
            let attribute = &self.0[..end];
            let (name, bound) = attribute
                .strip_prefix(':')
                .map_or((attribute, false), |name| (name, true));
            if !identifier(name) {
                return Err(invalid("Invalid Blade component attribute."));
            }
            self.0 = self.0[end..].trim_start();
            let value = if let Some(rest) = self.0.strip_prefix('=') {
                self.0 = rest.trim_start();
                let delimiter = match self.0.as_bytes().first() {
                    Some(b'\'') => "'",
                    Some(b'"') => "\"",
                    _ => return Err(invalid("Blade component attributes require quotes.")),
                };
                let content = self.delimited(1, delimiter)?;
                if bound {
                    expression_value(content)?
                } else {
                    Expression::Value(content.into())
                }
            } else if bound {
                return Err(invalid("A bound Blade attribute requires a value."));
            } else {
                Expression::Value(true.into())
            };
            attributes.push((name.to_owned(), value));
        }
    }

    fn view_directive(&mut self, depth: usize) -> ViewResult<Option<Node>> {
        let node = if self.directive("parent") {
            Node::Parent
        } else if self.directive("extends") {
            Node::Extends(expression_value(self.arguments()?)?)
        } else if self.directive("include") {
            Node::Include(expression_value(self.arguments()?)?)
        } else if self.directive("yield") {
            let (name, default) = self.named_argument()?;
            Node::Yield(name, default)
        } else if self.directive("section") {
            let (name, content) = self.named_argument()?;
            match content {
                Some(content) => Node::Section(name, vec![Node::Echo(content, true)], false),
                None => {
                    let (body, stop) = self.block(depth + 1)?;
                    match stop {
                        Some(Stop::EndSection) => Node::Section(name, body, false),
                        Some(Stop::Show) => Node::Section(name, body, true),
                        _ => return Err(invalid("Expected @endsection or @show.")),
                    }
                }
            }
        } else {
            return Ok(None);
        };
        Ok(Some(node))
    }

    fn named_argument(&mut self) -> ViewResult<(Expression, Option<Expression>)> {
        let arguments = self.arguments()?;
        if let Some(index) = argument_separator(arguments) {
            let name = expression_value(&arguments[..index])?;
            let value = expression_value(&arguments[index + 1..])?;
            Ok((name, Some(value)))
        } else {
            Ok((expression_value(arguments)?, None))
        }
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
            ("endsection", Stop::EndSection),
            ("show", Stop::Show),
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
        let rest = self
            .0
            .trim_start()
            .strip_prefix('(')
            .ok_or_else(|| invalid("Blade directive requires parentheses."))?;
        let mut quote = Quote::default();
        let mut nested = 0usize;
        for (index, character) in rest.char_indices() {
            if quote.literal(character) {
                continue;
            }
            match character {
                '(' => nested += 1,
                ')' if nested == 0 => {
                    self.0 = &rest[index + 1..];
                    return Ok(&rest[..index]);
                }
                ')' => nested -= 1,
                _ => {}
            }
        }
        Err(invalid("Unclosed Blade expression."))
    }

    fn delimited(&mut self, prefix: usize, delimiter: &str) -> ViewResult<&'a str> {
        let (value, rest) = self.0[prefix..]
            .split_once(delimiter)
            .ok_or_else(|| invalid("Unclosed Blade expression."))?;
        self.0 = rest;
        Ok(value)
    }
}

#[derive(Default)]
struct Quote {
    delimiter: Option<char>,
    escaped: bool,
}

impl Quote {
    fn literal(&mut self, character: char) -> bool {
        if let Some(delimiter) = self.delimiter {
            if self.escaped {
                self.escaped = false;
            } else if character == '\\' {
                self.escaped = true;
            } else if character == delimiter {
                self.delimiter = None;
            }
            return true;
        }
        if matches!(character, '\'' | '"') {
            self.delimiter = Some(character);
            return true;
        }
        false
    }
}

fn argument_separator(arguments: &str) -> Option<usize> {
    let mut quote = Quote::default();
    arguments.char_indices().find_map(|(index, character)| {
        (!quote.literal(character) && character == ',').then_some(index)
    })
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
    context: &mut RenderContext<'_>,
) -> ViewResult<()> {
    for node in nodes {
        match node {
            Node::Text(text) => output.push_str(text),
            Node::Echo(expression, escape) => {
                if let Expression::Variable(path) = expression
                    && let [name] = path.as_slice()
                    && !bindings.iter().any(|(key, _)| *key == name)
                    && let Some(slot) = context.slot(name)
                {
                    output.push_str(slot);
                } else {
                    echo(output, value(expression, data, bindings), *escape);
                }
            }
            Node::Conditional(expression, predicate, yes, no) => {
                let value = value(expression, data, bindings);
                let matches = match predicate {
                    Predicate::Truthy => truthy(value),
                    Predicate::Empty => !truthy(value),
                    Predicate::Isset => !value.is_null(),
                };
                render_nodes(
                    if matches { yes } else { no },
                    data,
                    bindings,
                    output,
                    context,
                )?;
            }
            Node::Each(expression, variable, body, alternative) => {
                let value = value(expression, data, bindings);
                let arrays = value.as_array().into_iter().flatten();
                let objects = value.as_object().into_iter().flat_map(|map| map.values());
                let mut values = arrays.chain(objects).peekable();
                if values.peek().is_none() {
                    render_nodes(alternative, data, bindings, output, context)?;
                }
                for value in values {
                    bindings.push((variable, value));
                    render_nodes(body, data, bindings, output, context)?;
                    bindings.pop();
                }
            }
            Node::Parent => output.push_str(context.parent()),
            Node::Extends(expression) => {
                context.extend(name_value(value(expression, data, bindings))?.to_owned())?;
            }
            Node::Section(expression, nodes, show) => {
                let name = name_value(value(expression, data, bindings))?.to_owned();
                let mut section = String::new();
                render_nodes(nodes, data, bindings, &mut section, context)?;
                context.section(name.clone(), section);
                if *show {
                    context.yield_to(&name, output);
                }
            }
            Node::Yield(expression, default) => {
                let name = name_value(value(expression, data, bindings))?;
                if !context.yield_to(name, output)
                    && let Some(default) = default
                {
                    echo(output, value(default, data, bindings), true);
                }
            }
            Node::Include(expression) => {
                let name = name_value(value(expression, data, bindings))?;
                let mut local_data = data.clone();
                local_data.extend(
                    bindings
                        .iter()
                        .map(|(key, value)| ((*key).to_owned(), (*value).clone())),
                );
                output.push_str(&context.include(name, &local_data)?);
            }
            Node::Component {
                name,
                attributes,
                body,
                slots,
            } => {
                let properties = attributes
                    .iter()
                    .map(|(name, expression)| {
                        (name.clone(), value(expression, data, bindings).clone())
                    })
                    .collect();
                let mut default = String::new();
                render_nodes(body, data, bindings, &mut default, context)?;
                let mut rendered = std::collections::HashMap::from([("slot".to_owned(), default)]);
                for (name, nodes) in slots {
                    let mut html = String::new();
                    render_nodes(nodes, data, bindings, &mut html, context)?;
                    rendered.insert(name.clone(), html);
                }
                output.push_str(&context.component(name, &properties, rendered)?);
            }
            Node::Slot(_, _) => return Err(render_error("Blade named slots require a component.")),
        }
    }
    Ok(())
}

fn name_value(value: &ViewValue) -> ViewResult<&str> {
    value
        .as_str()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| render_error("Blade directive requires a non-empty name."))
}

fn echo(output: &mut String, value: &ViewValue, escape: bool) {
    match value {
        ViewValue::String(string) if escape => escape_html(output, string),
        ViewValue::String(string) => output.push_str(string),
        ViewValue::Bool(true) => output.push('1'),
        ViewValue::Number(number) => write!(output, "{number}").unwrap(),
        _ => {}
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

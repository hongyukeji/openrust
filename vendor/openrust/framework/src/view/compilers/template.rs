use std::{borrow::Cow, fmt::Write as _, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{DirectiveOutput, Directives};
use crate::view::{
    ViewData, ViewError, ViewResult, ViewValue,
    rendering::{RenderContext, render_error},
};

mod attributes;
mod expressions;
mod includes;
mod loops;

use attributes::{
    AttributeList, Props, apply_props, attribute_list, attribute_name, merge_attributes,
    parse_attribute_list, parse_echo, parse_props, render_attributes,
};

use expressions::{Expression, expression_value, truthy, value};
use includes::{Include, IncludeFirst, RenderEach, collection_entries};
use loops::{CountedLoop, Jump, JumpKind};

#[derive(Serialize, Deserialize)]
pub(super) struct Template {
    nodes: Vec<Node>,
    props: Option<Props>,
}

#[derive(Serialize, Deserialize)]
enum Node {
    Text(String),
    Echo(Expression, bool),
    Attributes(Vec<(String, Expression)>),
    Conditional(Expression, Predicate, Vec<Node>, Vec<Node>),
    Each {
        expression: Expression,
        key: Option<String>,
        item: String,
        body: Vec<Node>,
        alternative: Vec<Node>,
    },
    For(CountedLoop),
    While(Expression, Vec<Node>),
    Jump(JumpKind, usize, Option<Expression>),
    Extends(Expression),
    Parent,
    Section(Expression, Vec<Node>, bool),
    Yield(Expression, Option<Expression>),
    Stack(Expression, Option<Expression>),
    StackCapture(Expression, Vec<Node>, bool),
    Once(OnceId, Vec<Node>),
    Csrf,
    Method(Expression),
    Error(Expression, Option<Expression>, Vec<Node>),
    Fragment(Expression, Vec<Node>),
    BooleanAttribute(String, Expression),
    ConditionalAttribute(AttributeList, bool),
    CustomDirective(String, Vec<Expression>),
    Include(Include, bool),
    IncludeWhen(Expression, Include, bool),
    IncludeFirst(IncludeFirst),
    IncludeIsolated(Include),
    RenderEach(RenderEach),
    Component {
        name: ComponentName,
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
    HasStack,
}

#[derive(Serialize, Deserialize)]
enum ComponentName {
    Static(String),
    Dynamic(Expression),
}

#[derive(Serialize, Deserialize)]
enum OnceId {
    Explicit(Expression),
    Generated(String),
}

impl Template {
    pub(super) fn compile(
        source: &str,
        identity: &str,
        directives: &Directives,
    ) -> ViewResult<Self> {
        let mut parser = Parser::new(source, identity, directives);
        let trimmed = source.trim_start();
        let mut leading = Parser::new(trimmed, identity, directives);
        let props = if leading.directive("props") {
            parser = leading;
            Some(parse_props(parser.arguments()?)?)
        } else {
            None
        };
        let (mut nodes, stop) = parser.block(0)?;
        if stop.is_some() {
            return Err(invalid("Unexpected closing Blade directive."));
        }
        if props.is_some() && source.len() != trimmed.len() {
            nodes.insert(
                0,
                Node::Text(source[..source.len() - trimmed.len()].to_owned()),
            );
        }
        Ok(Self { nodes, props })
    }

    pub(super) fn render(
        &self,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let data = match &self.props {
            Some(props) => Cow::Owned(apply_props(data, props, context)),
            None => Cow::Borrowed(data),
        };
        let mut output = String::new();
        if render_nodes(&self.nodes, &data, &[], &mut output, context)?.is_some() {
            return Err(render_error(
                "Blade loop control exceeded its enclosing blocks.",
            ));
        }
        output.push_str(&context.render_layout(&data)?);
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
    EndFor,
    EndWhile,
    Empty,
    EndEmpty,
    EndIsset,
    EndSection,
    Show,
    EndPush,
    EndPrepend,
    EndOnce,
    EndPushOnce,
    EndPrependOnce,
    EndError,
    EndFragment,
    EndTag,
}

struct Parser<'a> {
    remaining: &'a str,
    identity: &'a str,
    directives: &'a Directives,
    once_count: usize,
    loop_depth: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, identity: &'a str, directives: &'a Directives) -> Self {
        Self {
            remaining: source,
            identity,
            directives,
            once_count: 0,
            loop_depth: 0,
        }
    }

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
        while !self.remaining.is_empty() {
            let index = self
                .remaining
                .find(['@', '{', '<'])
                .unwrap_or(self.remaining.len());
            text.push_str(&self.remaining[..index]);
            self.remaining = &self.remaining[index..];
            if self.remaining.is_empty() {
                break;
            }
            if let Some(rest) = self.remaining.strip_prefix("</x-") {
                let (name, tail) = rest
                    .split_once('>')
                    .ok_or_else(|| invalid("Unclosed Blade component closing tag."))?;
                let name = name.trim_end();
                if closing != Some(name)
                    && !(name == "slot" && closing.is_some_and(|tag| tag.starts_with("slot:")))
                {
                    return Err(invalid("Mismatched Blade component closing tag."));
                }
                self.remaining = tail;
                push_text(&mut nodes, &mut text);
                return Ok((nodes, Some(Stop::EndTag)));
            } else if self.remaining.starts_with("<x-") {
                push_text(&mut nodes, &mut text);
                nodes.push(self.component(depth)?);
            } else if self.remaining.starts_with("@{{") || self.remaining.starts_with("@{!!") {
                self.remaining = &self.remaining[1..];
                let end = if self.remaining.starts_with("{{") {
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
            } else if self.remaining.starts_with("@@") {
                text.push('@');
                self.remaining = &self.remaining[2..];
            } else if self.remaining.starts_with("{{--") {
                self.delimited(4, "--}}")?;
            } else if self.remaining.starts_with("{{") || self.remaining.starts_with("{!!") {
                push_text(&mut nodes, &mut text);
                let escaped = self.remaining.starts_with("{{");
                let expression = if escaped {
                    self.delimited(2, "}}")?
                } else {
                    self.delimited(3, "!!}")?
                };
                nodes.push(parse_echo(expression, escaped)?);
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
                let character = self.remaining.chars().next().unwrap();
                text.push(character);
                self.remaining = &self.remaining[character.len_utf8()..];
            }
        }
        push_text(&mut nodes, &mut text);
        Ok((nodes, None))
    }

    fn component(&mut self, depth: usize) -> ViewResult<Node> {
        self.remaining = &self.remaining[3..];
        let end = self
            .remaining
            .find(|character: char| character.is_whitespace() || matches!(character, '>' | '/'))
            .ok_or_else(|| invalid("Unclosed Blade component tag."))?;
        let name = &self.remaining[..end];
        self.remaining = &self.remaining[end..];
        let (mut attributes, self_closing) = self.component_attributes()?;

        if let Some(slot) = name.strip_prefix("slot:") {
            if !identifier(slot) || !attributes.is_empty() || self_closing {
                return Err(invalid("Invalid Blade named slot."));
            }
            return Ok(Node::Slot(
                slot.to_owned(),
                self.component_body(depth, name)?,
            ));
        }
        if !valid_component_name(name) {
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
        let name = if name == "dynamic-component" {
            let index = attributes
                .iter()
                .position(|(name, _)| name == "component")
                .ok_or_else(|| {
                    invalid("A dynamic Blade component requires a component attribute.")
                })?;
            let (_, target) = attributes.remove(index);
            if attributes.iter().any(|(name, _)| name == "component") {
                return Err(invalid(
                    "A dynamic Blade component accepts one component attribute.",
                ));
            }
            ComponentName::Dynamic(target)
        } else {
            ComponentName::Static(name.to_owned())
        };
        Ok(Node::Component {
            name,
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
            self.remaining = self.remaining.trim_start();
            if let Some(rest) = self.remaining.strip_prefix("/>") {
                self.remaining = rest;
                return Ok((attributes, true));
            }
            if let Some(rest) = self.remaining.strip_prefix('>') {
                self.remaining = rest;
                return Ok((attributes, false));
            }
            let end = self
                .remaining
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, '=' | '>' | '/')
                })
                .ok_or_else(|| invalid("Unclosed Blade component tag."))?;
            let attribute = &self.remaining[..end];
            let (name, bound) = attribute
                .strip_prefix(':')
                .map_or((attribute, false), |name| (name, true));
            if !attribute_name(name) {
                return Err(invalid("Invalid Blade component attribute."));
            }
            self.remaining = self.remaining[end..].trim_start();
            let value = if let Some(rest) = self.remaining.strip_prefix('=') {
                self.remaining = rest.trim_start();
                let delimiter = match self.remaining.as_bytes().first() {
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
        let node = if self.directive("props") {
            return Err(invalid("Place @props at the start of the Blade template."));
        } else if self.directive("parent") {
            Node::Parent
        } else if self.directive("extends") {
            Node::Extends(expression_value(self.arguments()?)?)
        } else if self.directive("include") {
            Node::Include(Include::parse(self.arguments()?)?, false)
        } else if self.directive("includeIf") {
            Node::Include(Include::parse(self.arguments()?)?, true)
        } else if self.directive("includeWhen") {
            self.conditional_include(true)?
        } else if self.directive("includeUnless") {
            self.conditional_include(false)?
        } else if self.directive("includeFirst") {
            Node::IncludeFirst(IncludeFirst::parse(self.arguments()?)?)
        } else if self.directive("includeIsolated") {
            Node::IncludeIsolated(Include::parse(self.arguments()?)?)
        } else if self.directive("each") {
            Node::RenderEach(RenderEach::parse(self.arguments()?)?)
        } else if self.directive("for") {
            let mut counted = CountedLoop::parse(self.arguments()?)?;
            let (body, stop) = self.around_loop(depth)?;
            if stop != Some(Stop::EndFor) {
                return Err(invalid("Expected @endfor."));
            }
            counted.body = body;
            Node::For(counted)
        } else if self.directive("while") {
            let condition = expression_value(self.arguments()?)?;
            let (body, stop) = self.around_loop(depth)?;
            if stop != Some(Stop::EndWhile) {
                return Err(invalid("Expected @endwhile."));
            }
            Node::While(condition, body)
        } else if self.directive("break") {
            self.jump(JumpKind::Break)?
        } else if self.directive("continue") {
            self.jump(JumpKind::Continue)?
        } else if self.directive("yield") {
            let (name, default) = self.named_argument()?;
            Node::Yield(name, default)
        } else if self.directive("stack") {
            let (name, default) = self.named_argument()?;
            Node::Stack(name, default)
        } else if self.directive("push") {
            self.capture_stack(depth, false)?
        } else if self.directive("prepend") {
            self.capture_stack(depth, true)?
        } else if self.directive("once") {
            let id = if self.remaining.trim_start().starts_with('(') {
                Some(expression_value(self.arguments()?)?)
            } else {
                None
            };
            let id = self.once_id(id);
            Node::Once(id, self.block_ending(depth, Stop::EndOnce)?)
        } else if self.directive("pushOnce") {
            self.capture_once(depth, false)?
        } else if self.directive("prependOnce") {
            self.capture_once(depth, true)?
        } else if self.directive("csrf") {
            Node::Csrf
        } else if self.directive("method") {
            Node::Method(expression_value(self.arguments()?)?)
        } else if self.directive("error") {
            let (field, bag) = self.named_argument()?;
            let (body, stop) = self.block(depth + 1)?;
            if stop != Some(Stop::EndError) {
                return Err(invalid("Expected @enderror."));
            }
            Node::Error(field, bag, body)
        } else if self.directive("fragment") {
            let name = expression_value(self.arguments()?)?;
            let (body, stop) = self.block(depth + 1)?;
            if stop != Some(Stop::EndFragment) {
                return Err(invalid("Expected @endfragment."));
            }
            Node::Fragment(name, body)
        } else if let Some(name) = ["checked", "selected", "disabled", "readonly", "required"]
            .into_iter()
            .find(|name| self.directive(name))
        {
            Node::BooleanAttribute(name.to_owned(), expression_value(self.arguments()?)?)
        } else if let Some(style) = ["class", "style"]
            .into_iter()
            .find(|name| self.directive(name))
        {
            Node::ConditionalAttribute(parse_attribute_list(self.arguments()?)?, style == "style")
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
            return self.custom_directive();
        };
        Ok(Some(node))
    }

    fn custom_directive(&mut self) -> ViewResult<Option<Node>> {
        let Some(rest) = self.remaining.strip_prefix('@') else {
            return Ok(None);
        };
        let end = rest
            .find(|character: char| !(character.is_alphanumeric() || character == '_'))
            .unwrap_or(rest.len());
        let (name, rest) = rest.split_at(end);
        if !self.directives.contains_key(name) {
            return Ok(None);
        }
        self.remaining = rest;
        let mut arguments = Vec::new();
        if self.remaining.trim_start().starts_with('(') {
            let input = self.arguments()?.trim();
            if !input.is_empty() {
                let mut start = 0;
                for end in separators(input, ",").chain(std::iter::once(input.len())) {
                    arguments.push(expression_value(&input[start..end])?);
                    start = end + 1;
                }
            }
        }
        Ok(Some(Node::CustomDirective(name.to_owned(), arguments)))
    }

    fn capture_stack(&mut self, depth: usize, prepend: bool) -> ViewResult<Node> {
        let (name, content) = self.named_argument()?;
        let body = match content {
            Some(content) => vec![Node::Echo(content, false)],
            None => self.block_ending(
                depth,
                if prepend {
                    Stop::EndPrepend
                } else {
                    Stop::EndPush
                },
            )?,
        };
        Ok(Node::StackCapture(name, body, prepend))
    }

    fn capture_once(&mut self, depth: usize, prepend: bool) -> ViewResult<Node> {
        let (name, id) = self.named_argument()?;
        let id = self.once_id(id);
        let expected = if prepend {
            Stop::EndPrependOnce
        } else {
            Stop::EndPushOnce
        };
        let capture = Node::StackCapture(name, self.block_ending(depth, expected)?, prepend);
        Ok(Node::Once(id, vec![capture]))
    }

    fn once_id(&mut self, explicit: Option<Expression>) -> OnceId {
        match explicit {
            Some(explicit) => OnceId::Explicit(explicit),
            None => {
                self.once_count += 1;
                OnceId::Generated(format!("{}:{}", self.identity, self.once_count))
            }
        }
    }

    fn block_ending(&mut self, depth: usize, expected: Stop) -> ViewResult<Vec<Node>> {
        let (nodes, stop) = self.block(depth + 1)?;
        if stop != Some(expected) {
            return Err(invalid("Unclosed Blade stack or once block."));
        }
        Ok(nodes)
    }

    fn conditional_include(&mut self, when: bool) -> ViewResult<Node> {
        let args = self.arguments()?;
        let separator = argument_separator(args)
            .ok_or_else(|| invalid("Conditional Blade include requires a view name."))?;
        let condition = expression_value(&args[..separator])?;
        let include = Include::parse(&args[separator + 1..])?;
        Ok(Node::IncludeWhen(condition, include, when))
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
            ("hasStack", Predicate::HasStack, Stop::EndIf),
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
        let separator = arguments
            .rmatch_indices("as")
            .find(|(index, _)| {
                arguments[..*index].ends_with(char::is_whitespace)
                    && arguments[*index + 2..].starts_with(char::is_whitespace)
            })
            .map(|(index, _)| index)
            .ok_or_else(|| invalid("Expected a Blade loop of the form $items as $item."))?;
        let expression = expression_value(&arguments[..separator])?;
        let binding = &arguments[separator + 2..];
        let (key, item) = match binding.split_once("=>") {
            Some((key, item)) => (Some(loop_variable(key)?), loop_variable(item)?),
            None => (None, loop_variable(binding)?),
        };
        let (body, stop) = self.around_loop(depth)?;
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
        Ok(Node::Each {
            expression,
            key,
            item,
            body,
            alternative,
        })
    }

    fn around_loop(&mut self, depth: usize) -> ViewResult<(Vec<Node>, Option<Stop>)> {
        self.loop_depth += 1;
        let result = self.block(depth + 1);
        self.loop_depth -= 1;
        result
    }

    fn jump(&mut self, kind: JumpKind) -> ViewResult<Node> {
        let (levels, condition) = if self.remaining.trim_start().starts_with('(') {
            let expression = self.arguments()?.trim();
            match expression.parse::<i64>() {
                Ok(levels) => (usize::try_from(levels.max(1)).unwrap_or(usize::MAX), None),
                Err(_) => (1, Some(expression_value(expression)?)),
            }
        } else {
            (1, None)
        };
        if levels > self.loop_depth {
            return Err(invalid("Blade loop control exceeds its enclosing blocks."));
        }
        Ok(Node::Jump(kind, levels, condition))
    }

    fn stop(&mut self) -> Option<Stop> {
        if let Some(rest) = self.remaining.strip_prefix("@empty")
            && !rest.trim_start().starts_with('(')
            && self.directive("empty")
        {
            return Some(Stop::Empty);
        }
        [
            ("endsection", Stop::EndSection),
            ("show", Stop::Show),
            ("endpush", Stop::EndPush),
            ("endprepend", Stop::EndPrepend),
            ("endonce", Stop::EndOnce),
            ("endPushOnce", Stop::EndPushOnce),
            ("endPrependOnce", Stop::EndPrependOnce),
            ("enderror", Stop::EndError),
            ("endfragment", Stop::EndFragment),
            ("endif", Stop::EndIf),
            ("endunless", Stop::EndUnless),
            ("endforeach", Stop::EndForeach),
            ("endforelse", Stop::EndForelse),
            ("endfor", Stop::EndFor),
            ("endwhile", Stop::EndWhile),
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
            .remaining
            .strip_prefix('@')
            .and_then(|rest| rest.strip_prefix(name))
        else {
            return false;
        };
        if rest.starts_with(|character: char| character.is_alphanumeric() || character == '_') {
            return false;
        }
        self.remaining = rest;
        true
    }

    fn arguments(&mut self) -> ViewResult<&'a str> {
        let rest = self
            .remaining
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
                    self.remaining = &rest[index + 1..];
                    return Ok(&rest[..index]);
                }
                ')' => nested -= 1,
                _ => {}
            }
        }
        Err(invalid("Unclosed Blade expression."))
    }

    fn delimited(&mut self, prefix: usize, delimiter: &str) -> ViewResult<&'a str> {
        let (value, rest) = self.remaining[prefix..]
            .split_once(delimiter)
            .ok_or_else(|| invalid("Unclosed Blade expression."))?;
        self.remaining = rest;
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
    separators(arguments, ",").next()
}

fn separators<'a>(source: &'a str, pattern: &'a str) -> impl Iterator<Item = usize> + 'a {
    let mut quote = Quote::default();
    let mut depth = 0usize;
    source.char_indices().filter_map(move |(index, character)| {
        if quote.literal(character) {
            return None;
        }
        match character {
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && source[index..].starts_with(pattern) => return Some(index),
            _ => {}
        }
        None
    })
}

fn push_text(nodes: &mut Vec<Node>, text: &mut String) {
    if !text.is_empty() {
        nodes.push(Node::Text(std::mem::take(text)));
    }
}

pub(super) fn identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && characters.all(|character| character.is_alphanumeric() || character == '_')
}

fn valid_component_name(name: &str) -> bool {
    name.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
    })
}

fn loop_variable(binding: &str) -> ViewResult<String> {
    binding
        .trim()
        .strip_prefix('$')
        .filter(|name| identifier(name))
        .map(str::to_owned)
        .ok_or_else(|| invalid("Invalid Blade loop variable."))
}

macro_rules! render_block {
    ($($argument:expr),+ $(,)?) => {
        if let Some(jump) = render_nodes($($argument),+)? {
            return Ok(Some(jump));
        }
    };
}

fn render_nodes<'a>(
    nodes: &'a [Node],
    data: &'a ViewData,
    bindings: &[(&'a str, &'a ViewValue)],
    output: &mut String,
    context: &mut RenderContext<'_>,
) -> ViewResult<Option<Jump>> {
    for node in nodes {
        match node {
            Node::Text(text) => output.push_str(text),
            Node::Echo(expression, escape) => {
                if let Expression::Variable(path) = expression
                    && let [name] = path.as_slice()
                    && !bindings.iter().any(|(key, _)| *key == name)
                {
                    if let Some(slot) = context.slot(name) {
                        output.push_str(slot);
                        continue;
                    }
                    if name == "attributes"
                        && let Some(attributes) = &context.component_attributes
                    {
                        render_attributes(attributes, output)?;
                        continue;
                    }
                }
                echo(output, value(expression, data, bindings), *escape);
            }
            Node::Attributes(defaults) => {
                let attributes = context
                    .component_attributes
                    .as_ref()
                    .ok_or_else(|| render_error("A Blade component attribute bag is required."))?;
                render_attributes(
                    &merge_attributes(attributes, defaults, data, bindings),
                    output,
                )?;
            }
            Node::Conditional(expression, predicate, yes, no) => {
                let value = value(expression, data, bindings);
                let matches = match predicate {
                    Predicate::Truthy => truthy(value),
                    Predicate::Empty => !truthy(value),
                    Predicate::Isset => !value.is_null(),
                    Predicate::HasStack => context.has_stack(name_value(value)?),
                };
                render_block!(
                    if matches { yes } else { no },
                    data,
                    bindings,
                    output,
                    context,
                );
            }
            Node::Each {
                expression,
                key,
                item,
                body,
                alternative,
            } => {
                let collection = value(expression, data, bindings);
                let count = collection
                    .as_array()
                    .map_or_else(|| collection.as_object().map_or(0, ViewData::len), Vec::len);
                if count == 0 {
                    render_block!(alternative, data, bindings, output, context);
                }
                let parent = context.active_loop.clone();
                let depth = parent
                    .as_deref()
                    .and_then(|metadata| metadata["depth"].as_u64())
                    .unwrap_or(0)
                    + 1;
                for (index, (key_value, item_value)) in collection_entries(collection).enumerate() {
                    let iteration = index + 1;
                    let even = iteration.is_multiple_of(2);
                    let metadata = Arc::new(json!({
                        "iteration": iteration,
                        "index": index,
                        "remaining": count - iteration,
                        "count": count,
                        "first": index == 0,
                        "last": iteration == count,
                        "odd": !even,
                        "even": even,
                        "depth": depth,
                        "parent": parent.as_deref(),
                    }));
                    let mut iteration_bindings = bindings.to_vec();
                    if let Some(key) = key {
                        iteration_bindings.push((key, &key_value));
                    }
                    iteration_bindings.push((item, item_value));
                    iteration_bindings.push(("loop", &metadata));
                    context.active_loop = Some(metadata.clone());
                    let result = render_nodes(body, data, &iteration_bindings, output, context);
                    context.active_loop = parent.clone();
                    match result? {
                        Some(jump) if jump.levels > 1 => return Ok(Some(jump.outer())),
                        Some(Jump {
                            kind: JumpKind::Break,
                            ..
                        }) => break,
                        _ => {}
                    }
                }
            }
            Node::For(counted) => {
                let mut counter = loop_integer(value(&counted.start, data, bindings))?;
                loop {
                    let current = ViewValue::from(counter);
                    let mut locals = bindings.to_vec();
                    locals.push((&counted.variable, &current));
                    if !truthy(value(&counted.condition, data, &locals)) {
                        break;
                    }
                    context.tick_loop()?;
                    match render_nodes(&counted.body, data, &locals, output, context)? {
                        Some(jump) if jump.levels > 1 => return Ok(Some(jump.outer())),
                        Some(Jump {
                            kind: JumpKind::Break,
                            ..
                        }) => break,
                        _ => {}
                    }
                    let step = loop_integer(value(&counted.step, data, &locals))?;
                    counter = if counted.descending {
                        counter.checked_sub(step)
                    } else {
                        counter.checked_add(step)
                    }
                    .ok_or_else(|| render_error("Blade @for counter overflow."))?;
                }
            }
            Node::While(condition, body) => {
                while truthy(value(condition, data, bindings)) {
                    context.tick_loop()?;
                    match render_nodes(body, data, bindings, output, context)? {
                        Some(jump) if jump.levels > 1 => return Ok(Some(jump.outer())),
                        Some(Jump {
                            kind: JumpKind::Break,
                            ..
                        }) => break,
                        _ => {}
                    }
                }
            }
            Node::Jump(kind, levels, condition) => {
                if condition
                    .as_ref()
                    .is_none_or(|condition| truthy(value(condition, data, bindings)))
                {
                    return Ok(Some(Jump {
                        kind: *kind,
                        levels: *levels,
                    }));
                }
            }
            Node::Parent => output.push_str(context.parent()),
            Node::Extends(expression) => {
                context.extend(name_value(value(expression, data, bindings))?.to_owned())?;
            }
            Node::Section(expression, nodes, show) => {
                let name = name_value(value(expression, data, bindings))?.to_owned();
                let mut section = String::new();
                render_block!(nodes, data, bindings, &mut section, context);
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
            Node::Stack(expression, default) => {
                let name = name_value(value(expression, data, bindings))?;
                if !context.stack(name, output)
                    && let Some(default) = default
                {
                    echo(output, value(default, data, bindings), false);
                }
            }
            Node::StackCapture(expression, nodes, prepend) => {
                let name = name_value(value(expression, data, bindings))?.to_owned();
                let mut content = String::new();
                render_block!(nodes, data, bindings, &mut content, context);
                context.add_to_stack(name, content, *prepend);
            }
            Node::Once(id, nodes) => {
                let (id, generated) = match id {
                    OnceId::Generated(id) => (id.as_str(), true),
                    OnceId::Explicit(expression) => {
                        (name_value(value(expression, data, bindings))?, false)
                    }
                };
                if context.first_render_of(id, generated) {
                    render_block!(nodes, data, bindings, output, context);
                }
            }
            Node::Csrf => {
                let token = context.csrf_token.as_deref().ok_or_else(|| {
                    render_error("The Blade CSRF field requires an active session.")
                })?;
                hidden_field(output, "_token", token);
            }
            Node::Method(expression) => {
                hidden_field(
                    output,
                    "_method",
                    name_value(value(expression, data, bindings))?,
                );
            }
            Node::Error(field, bag, nodes) => {
                let field = name_value(value(field, data, bindings))?;
                let mut bag_name = "default";
                if let Some(bag) = bag {
                    bag_name = name_value(value(bag, data, bindings))?;
                }
                if let Some(message) = data
                    .get("errors")
                    .and_then(|errors| errors.get(bag_name))
                    .and_then(|errors| errors.get(field))
                    .map(|message| message.get(0).unwrap_or(message))
                    .filter(|message| message.as_str().is_some_and(|text| !text.is_empty()))
                {
                    let mut local = bindings.to_vec();
                    local.push(("message", message));
                    render_block!(nodes, data, &local, output, context);
                }
            }
            Node::Fragment(name, nodes) => {
                let name = name_value(value(name, data, bindings))?.to_owned();
                let mut html = String::new();
                render_block!(nodes, data, bindings, &mut html, context);
                output.push_str(&html);
                context.capture_fragment(name, html);
            }
            Node::BooleanAttribute(name, condition) => {
                if truthy(value(condition, data, bindings)) {
                    output.push_str(name);
                }
            }
            Node::ConditionalAttribute(entries, style) => {
                let name = if *style { "style" } else { "class" };
                let text = attribute_list(entries, data, bindings, *style)?;
                write!(output, "{name}=\"").unwrap();
                escape_html(output, &text);
                output.push('"');
            }
            Node::CustomDirective(name, arguments) => {
                let callback = context
                    .directives
                    .get(name)
                    .ok_or_else(|| render_error("Blade directive registration was not found."))?;
                let arguments = arguments
                    .iter()
                    .map(|argument| value(argument, data, bindings).clone())
                    .collect::<Vec<_>>();
                match callback(&arguments)? {
                    DirectiveOutput::Text(text) => escape_html(output, &text),
                    DirectiveOutput::Html(html) => output.push_str(&html),
                }
            }
            Node::Include(include, optional) => {
                let name = include.name(data, bindings)?;
                if !optional || context.exists(name)? {
                    output.push_str(&context.include(name, &include.data(data, bindings, true)?)?);
                }
            }
            Node::IncludeWhen(condition, include, when) => {
                if truthy(value(condition, data, bindings)) == *when {
                    let name = include.name(data, bindings)?;
                    output.push_str(&context.include(name, &include.data(data, bindings, true)?)?);
                }
            }
            Node::IncludeFirst(include) => {
                let names = include.names(data, bindings)?;
                output.push_str(&context.include_first(&names, &include.data(data, bindings)?)?);
            }
            Node::IncludeIsolated(include) => {
                let name = include.name(data, bindings)?;
                output.push_str(
                    &context.include_isolated(name, &include.data(data, bindings, false)?)?,
                );
            }
            Node::RenderEach(each) => each.render(data, bindings, output, context)?,
            Node::Component {
                name,
                attributes,
                body,
                slots,
            } => {
                let name = match name {
                    ComponentName::Static(name) => name.as_str(),
                    ComponentName::Dynamic(expression) => value(expression, data, bindings)
                        .as_str()
                        .filter(|name| valid_component_name(name))
                        .ok_or_else(|| render_error("Invalid dynamic Blade component name."))?,
                };
                let properties = attributes
                    .iter()
                    .map(|(name, expression)| {
                        (name.clone(), value(expression, data, bindings).clone())
                    })
                    .collect();
                let mut default = String::new();
                render_block!(body, data, bindings, &mut default, context);
                let mut rendered = std::collections::HashMap::from([("slot".to_owned(), default)]);
                for (name, nodes) in slots {
                    let mut html = String::new();
                    render_block!(nodes, data, bindings, &mut html, context);
                    rendered.insert(name.clone(), html);
                }
                output.push_str(&context.component(name, &properties, rendered)?);
            }
            Node::Slot(_, _) => return Err(render_error("Blade named slots require a component.")),
        }
    }
    Ok(None)
}

fn loop_integer(value: &ViewValue) -> ViewResult<i64> {
    value
        .as_i64()
        .ok_or_else(|| render_error("Blade @for requires a signed integer counter and step."))
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

fn hidden_field(output: &mut String, name: &str, value: &str) {
    write!(output, "<input type=\"hidden\" name=\"{name}\" value=\"").unwrap();
    escape_html(output, value);
    output.push_str("\">");
}

fn invalid(message: &'static str) -> ViewError {
    ViewError::CompileFailed(std::io::Error::new(std::io::ErrorKind::InvalidData, message).into())
}

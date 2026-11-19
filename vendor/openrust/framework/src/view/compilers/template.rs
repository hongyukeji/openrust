use std::{borrow::Cow, fmt::Write as _, sync::Arc};

use serde::{Deserialize, Serialize};

use super::{BladeExtensions, DirectiveOutput};
use crate::support::{Str, escape_html};
use crate::view::{
    ViewData, ViewError, ViewResult, ViewValue,
    rendering::{RenderContext, RenderSlot, render_error},
    request_data::AuthorizationMode,
};

mod arithmetic;
mod attributes;
mod authorization;
mod expressions;
mod fonts;
mod html;
mod includes;
mod javascript;
mod loops;
mod preparation;

use attributes::{
    AttributeChain, ComponentQuery, Props, apply_aware, apply_props, array_entries, attribute_name,
    css_list, merge_attributes, parse_component_query, parse_echo, parse_forwarded_bag,
    parse_props, render_attributes,
};

use authorization::AuthorizationBranch;
use expressions::{
    Expression, boolean_value, condition_value, evaluate, expression_value, loose_equal, truthy,
    value,
};
use includes::{Include, IncludeFirst, RenderEach, collection_entries, registered_iteration};
use loops::{CountedLoop, Jump, JumpKind};

#[derive(Serialize, Deserialize)]
pub(super) struct Template {
    nodes: Vec<Node>,
    declarations: Vec<Declaration>,
}

#[derive(Serialize, Deserialize)]
enum Declaration {
    Props(Props),
    Aware(Props),
    Inject(String, Expression),
}

#[derive(Serialize, Deserialize)]
enum Node {
    Text(String),
    Echo(Expression, bool),
    EscapedSectionValue(Expression),
    Attributes(AttributeChain),
    ComponentValue(ComponentQuery, bool),
    Fonts(fonts::Aliases),
    Conditional(ConditionalValue, Predicate, Vec<Node>, Vec<Node>),
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
    Switch(Expression, Vec<SwitchCase>),
    Extends(Include),
    ExtendsFirst(IncludeFirst),
    Parent,
    Section(Expression, Vec<Node>, SectionEnd),
    Yield(Expression, Option<Expression>),
    Stack(Expression, Option<Expression>),
    StackCapture(Expression, Vec<Node>, bool),
    Once(OnceId, Vec<Node>),
    Csrf,
    Method(Expression),
    Error(Expression, Option<Expression>, Vec<Node>),
    Fragment(Expression, Vec<Node>),
    BooleanAttribute(String, Expression),
    BooleanText(Expression),
    JavaScript(Expression),
    ConditionalAttribute(Expression, bool),
    CustomDirective(String, Vec<Expression>),
    CustomCondition(String, Vec<CustomBranch>, Vec<Node>),
    Environment(Vec<Expression>, Vec<Node>, Vec<Node>),
    Auth(Vec<AuthBranch>, Vec<Node>),
    Authorization(Vec<AuthorizationBranch>, Vec<Node>),
    RequestValue(RequestValueKind, Expression, Vec<Node>),
    Include(Include, bool),
    IncludeWhen(Expression, Include, bool),
    IncludeFirst(IncludeFirst),
    IncludeIsolated(Include),
    RenderEach(RenderEach),
    Component {
        name: ComponentName,
        attributes: Vec<TagAttribute>,
        body: Vec<Node>,
        slots: Vec<Slot>,
    },
    Slot(Slot),
}

#[derive(Serialize, Deserialize)]
enum TagAttribute {
    Value(String, Expression),
    Css(Expression, bool),
    Forward(AttributeChain),
}

#[derive(Serialize, Deserialize)]
struct Slot {
    name: Expression,
    attributes: Vec<TagAttribute>,
    body: Vec<Node>,
}

#[derive(Serialize, Deserialize)]
struct CustomBranch {
    arguments: Vec<Expression>,
    negated: bool,
    body: Vec<Node>,
}

#[derive(Serialize, Deserialize)]
struct AuthBranch {
    guard: Option<Expression>,
    guest: bool,
    body: Vec<Node>,
}

#[derive(Serialize, Deserialize)]
enum RequestValueKind {
    Session,
    Context,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum SectionEnd {
    Extend,
    Show,
    Append,
    Overwrite,
}

#[derive(Serialize, Deserialize)]
struct SwitchCase {
    value: Option<Expression>,
    body: Vec<Node>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
enum Predicate {
    Truthy,
    Empty,
    Isset,
    HasStack,
    HasSection,
    SectionMissing,
}

#[derive(Serialize, Deserialize)]
enum ConditionalValue {
    Expression(Expression),
    Component(ComponentQuery),
    Section(Expression, Option<Expression>),
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
        extensions: &BladeExtensions,
    ) -> ViewResult<Self> {
        let prepared = preparation::PreparedSource::new(source);
        let mut parser = Parser::new(&prepared.text, identity, extensions, &prepared.raw);
        let mut declarations = Vec::new();
        let mut whitespace = String::new();
        loop {
            let original = parser.remaining;
            let trimmed = original.trim_start();
            parser.remaining = trimmed;
            if parser.directive("props") {
                declarations.push(Declaration::Props(parse_props(&parser.arguments()?)?));
            } else if parser.directive("aware") {
                declarations.push(Declaration::Aware(parse_props(&parser.arguments()?)?));
            } else if parser.directive("inject") {
                let (variable, service) = parser.named_argument()?;
                let Expression::Value(ViewValue::String(variable)) = variable else {
                    return Err(invalid("Blade @inject requires a literal variable name."));
                };
                if !identifier(&variable) {
                    return Err(invalid("Invalid Blade @inject variable name."));
                }
                declarations.push(Declaration::Inject(
                    variable,
                    service.ok_or_else(|| {
                        invalid("Blade @inject requires a registered service name.")
                    })?,
                ));
            } else {
                parser.remaining = original;
                break;
            }
            whitespace.push_str(&original[..original.len() - trimmed.len()]);
        }
        let (mut nodes, stop) = parser.block(0)?;
        if stop.is_some() {
            return Err(invalid("Unexpected closing Blade directive."));
        }
        if !whitespace.is_empty() {
            nodes.insert(0, Node::Text(whitespace));
        }
        Ok(Self {
            nodes,
            declarations,
        })
    }

    pub(super) fn render(
        &self,
        data: &ViewData,
        context: &mut RenderContext<'_>,
    ) -> ViewResult<String> {
        let mut data = Cow::Borrowed(data);
        for declaration in &self.declarations {
            match declaration {
                Declaration::Props(props) => data = Cow::Owned(apply_props(&data, props, context)?),
                Declaration::Aware(aware) => apply_aware(data.to_mut(), aware, context)?,
                Declaration::Inject(variable, service) => {
                    let service = evaluated_name(service, &data, &[])?;
                    let exposed = context.view_service(&service)?;
                    data.to_mut().insert(variable.clone(), exposed);
                }
            }
        }
        let mut output = String::new();
        if render_nodes(&self.nodes, &data, &[], &mut output, context)?.is_some() {
            return Err(render_error(
                "Blade loop control exceeded its enclosing blocks.",
            ));
        }
        output.push_str(&context.render_layout()?);
        if output.contains(context.parent()) {
            output = output.replace(context.parent(), "");
        }
        Ok(output)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Stop<'a> {
    Else,
    ElseIf,
    EndIf,
    EndUnless,
    EndForeach,
    EndForelse,
    EndFor,
    EndWhile,
    Case,
    Default,
    EndSwitch,
    Empty,
    EndEmpty,
    EndIsset,
    EndSection,
    Append,
    Overwrite,
    Show,
    EndPush,
    EndPrepend,
    EndOnce,
    EndPushOnce,
    EndPrependOnce,
    ElsePushIf,
    EndPushIf,
    EndError,
    EndFragment,
    EndEnvironment,
    EndAuth,
    EndGuest,
    ElseAuth,
    ElseGuest,
    EndAuthorization(AuthorizationMode),
    ElseAuthorization(AuthorizationMode),
    EndProduction,
    EndSession,
    EndContext,
    EndTag,
    ElseCondition(&'a str),
    EndCondition(&'a str),
}

struct Parser<'a> {
    source: &'a str,
    arguments_available: bool,
    raw: &'a [String],
    remaining: &'a str,
    identity: &'a str,
    extensions: &'a BladeExtensions,
    once_count: usize,
    loop_depth: usize,
}

impl<'a> Parser<'a> {
    fn new(
        source: &'a str,
        identity: &'a str,
        extensions: &'a BladeExtensions,
        raw: &'a [String],
    ) -> Self {
        Self {
            source,
            arguments_available: false,
            raw,
            remaining: source,
            identity,
            extensions,
            once_count: 0,
            loop_depth: 0,
        }
    }

    fn block(&mut self, depth: usize) -> ViewResult<(Vec<Node>, Option<Stop<'a>>)> {
        self.block_until(depth, None)
    }

    fn block_until(
        &mut self,
        depth: usize,
        closing: Option<&str>,
    ) -> ViewResult<(Vec<Node>, Option<Stop<'a>>)> {
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
            } else if (self.remaining.starts_with("@{{") && self.remaining[3..].contains("}}"))
                || (self.remaining.starts_with("@{!!") && self.remaining[4..].contains("!!}"))
            {
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
            } else if self.statement_start()
                && self
                    .remaining
                    .strip_prefix("@@")
                    .and_then(|s| s.as_bytes().first())
                    .is_some_and(|b| statement_word(*b))
            {
                text.push('@');
                self.remaining = &self.remaining[2..];
            } else if (self.remaining.starts_with("{{") && self.remaining[2..].contains("}}"))
                || (self.remaining.starts_with("{!!") && self.remaining[3..].contains("!!}"))
            {
                push_text(&mut nodes, &mut text);
                let escaped = self.remaining.starts_with("{{");
                let expression = if escaped {
                    self.delimited(2, "}}")?
                } else {
                    self.delimited(3, "!!}")?
                };
                nodes.push(parse_echo(&expression, escaped)?);
            } else if let Some(rest) = self.remaining.strip_prefix("@__raw_block_")
                && let Some((index, tail)) = rest.split_once("__@")
                && let Ok(index) = index.parse::<usize>()
                && let Some(raw) = self.raw.get(index)
            {
                text.push_str(raw);
                self.remaining = tail;
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

        if name == "slot" || name.starts_with("slot:") {
            if self_closing {
                return Err(invalid("Invalid Blade named slot."));
            }
            let slot = if let Some(slot) = name.strip_prefix("slot:") {
                Expression::Value(slot.into())
            } else {
                take_tag_attribute(&mut attributes, "name")
                    .unwrap_or_else(|| Expression::Value("slot".into()))
            };
            return Ok(Node::Slot(Slot {
                name: slot,
                attributes,
                body: self.component_body(depth, name)?,
            }));
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
                Node::Slot(slot) => slots.push(slot),
                node => body.push(node),
            }
        }
        let name = if name == "dynamic-component" {
            let target = take_tag_attribute(&mut attributes, "component").ok_or_else(|| {
                invalid("A dynamic Blade component requires a component attribute.")
            })?;
            if take_tag_attribute(&mut attributes, "component").is_some() {
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

    fn component_attributes(&mut self) -> ViewResult<(Vec<TagAttribute>, bool)> {
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
            if let Some(style) = ["class", "style"]
                .into_iter()
                .find(|name| self.directive(name))
            {
                let expression = expression_value(&self.arguments()?)?;
                attributes.push(TagAttribute::Css(expression, style == "style"));
                continue;
            }
            if self.remaining.starts_with("{{") {
                let source = self.delimited(2, "}}")?;
                attributes.push(TagAttribute::Forward(parse_forwarded_bag(&source)?));
                continue;
            }
            let end = self
                .remaining
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, '=' | '>' | '/')
                })
                .ok_or_else(|| invalid("Unclosed Blade component tag."))?;
            let attribute = &self.remaining[..end];
            self.remaining = self.remaining[end..].trim_start();
            if let Some(short) = attribute.strip_prefix(":$") {
                if !identifier(short) || self.remaining.starts_with('=') {
                    return Err(invalid("Invalid Blade short attribute binding."));
                }
                attributes.push(TagAttribute::Value(
                    short.to_owned(),
                    Expression::Variable(vec![short.to_owned()]),
                ));
                continue;
            }
            let (name, bound) = if attribute.starts_with("::") {
                (&attribute[1..], false)
            } else {
                attribute
                    .strip_prefix(':')
                    .or_else(|| attribute.strip_prefix("bind:"))
                    .map_or((attribute, false), |name| (name, true))
            };
            if !attribute_name(name) {
                return Err(invalid("Invalid Blade component attribute."));
            }
            let value = if let Some(rest) = self.remaining.strip_prefix('=') {
                self.remaining = rest.trim_start();
                let delimiter = match self.remaining.as_bytes().first() {
                    Some(b'\'') => "'",
                    Some(b'"') => "\"",
                    _ => return Err(invalid("Blade component attributes require quotes.")),
                };
                let content = self.delimited(1, delimiter)?;
                if bound {
                    expression_value(&content)?
                } else {
                    Expression::Value(content.into_owned().into())
                }
            } else if bound {
                return Err(invalid("A bound Blade attribute requires a value."));
            } else {
                Expression::Value(true.into())
            };
            attributes.push(TagAttribute::Value(name.to_owned(), value));
        }
    }

    fn view_directive(&mut self, depth: usize) -> ViewResult<Option<Node>> {
        let node = if self.directive("php") || self.directive("endphp") || self.directive("use") {
            return Err(invalid(
                "PHP statements and imports must be implemented in application Rust code.",
            ));
        } else if self.directive("props") {
            return Err(invalid("Place @props at the start of the Blade template."));
        } else if self.directive("aware") {
            return Err(invalid("Place @aware at the start of the Blade template."));
        } else if self.directive("inject") {
            return Err(invalid("Place @inject at the start of the Blade template."));
        } else if self.directive("parent") {
            Node::Parent
        } else if self.directive("extends") {
            Node::Extends(Include::parse(&self.arguments()?)?)
        } else if self.directive("extendsFirst") {
            Node::ExtendsFirst(IncludeFirst::parse(&self.arguments()?)?)
        } else if self.directive("include") {
            Node::Include(Include::parse(&self.arguments()?)?, false)
        } else if self.directive("includeIf") {
            Node::Include(Include::parse(&self.arguments()?)?, true)
        } else if self.directive("includeWhen") {
            self.conditional_include(true)?
        } else if self.directive("includeUnless") {
            self.conditional_include(false)?
        } else if self.directive("includeFirst") {
            Node::IncludeFirst(IncludeFirst::parse(&self.arguments()?)?)
        } else if self.directive("includeIsolated") {
            Node::IncludeIsolated(Include::parse(&self.arguments()?)?)
        } else if self.directive("each") {
            Node::RenderEach(RenderEach::parse(&self.arguments()?)?)
        } else if self.directive("for") {
            let mut counted = CountedLoop::parse(&self.arguments()?)?;
            let (body, stop) = self.around_loop(depth)?;
            if stop != Some(Stop::EndFor) {
                return Err(invalid("Expected @endfor."));
            }
            counted.body = body;
            Node::For(counted)
        } else if self.directive("while") {
            let condition = expression_value(&self.arguments()?)?;
            let (body, stop) = self.around_loop(depth)?;
            if stop != Some(Stop::EndWhile) {
                return Err(invalid("Expected @endwhile."));
            }
            Node::While(condition, body)
        } else if self.directive("break") {
            self.jump(JumpKind::Break)?
        } else if self.directive("continue") {
            self.jump(JumpKind::Continue)?
        } else if self.directive("switch") {
            self.switch_block(depth)?
        } else if self.directive("bool") {
            Node::BooleanText(expression_value(&self.arguments()?)?)
        } else if self.directive("js") {
            Node::JavaScript(expression_value(&self.arguments()?)?)
        } else if self.directive("fonts") {
            let arguments = if self.arguments_available {
                self.arguments()?
            } else {
                Cow::Borrowed("")
            };
            fonts::parse(&arguments)?
        } else if self.directive("production") {
            self.environment_block(
                depth,
                vec![Expression::Value("production".into())],
                Stop::EndProduction,
            )?
        } else if self.directive("env") {
            let raw_arguments = self.arguments()?;
            let arguments = raw_arguments.trim();
            let patterns = if arguments.starts_with('[') && arguments.ends_with(']') {
                array_entries(arguments)?
                    .into_iter()
                    .map(expression_value)
                    .collect::<ViewResult<_>>()?
            } else {
                expression_arguments(arguments)?
            };
            self.environment_block(depth, patterns, Stop::EndEnvironment)?
        } else if self.directive("session") {
            self.request_value_block(depth, RequestValueKind::Session, Stop::EndSession)?
        } else if self.directive("context") {
            self.request_value_block(depth, RequestValueKind::Context, Stop::EndContext)?
        } else if self.directive("auth") {
            self.auth_block(depth, false, Stop::EndAuth)?
        } else if self.directive("guest") {
            self.auth_block(depth, true, Stop::EndGuest)?
        } else if self.directive("can") {
            self.authorization_block(depth, AuthorizationMode::All)?
        } else if self.directive("cannot") {
            self.authorization_block(depth, AuthorizationMode::Denied)?
        } else if self.directive("canany") {
            self.authorization_block(depth, AuthorizationMode::Any)?
        } else if self.directive("yield") {
            let (name, default) = self.named_argument()?;
            Node::Yield(name, default)
        } else if self.directive("stack") {
            let (name, default) = self.named_argument()?;
            Node::Stack(name, default)
        } else if self.directive("push") {
            self.capture_stack(depth, false)?
        } else if self.directive("pushIf") {
            self.capture_stack_if(depth)?
        } else if self.directive("prepend") {
            self.capture_stack(depth, true)?
        } else if self.directive("once") {
            let id = if self.arguments_available {
                Some(expression_value(&self.arguments()?)?)
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
            Node::Method(expression_value(&self.arguments()?)?)
        } else if self.directive("error") {
            let (field, bag) = self.named_argument()?;
            let (body, stop) = self.block(depth + 1)?;
            if stop != Some(Stop::EndError) {
                return Err(invalid("Expected @enderror."));
            }
            Node::Error(field, bag, body)
        } else if self.directive("fragment") {
            let name = expression_value(&self.arguments()?)?;
            let (body, stop) = self.block(depth + 1)?;
            if stop != Some(Stop::EndFragment) {
                return Err(invalid("Expected @endfragment."));
            }
            Node::Fragment(name, body)
        } else if let Some(name) = ["checked", "selected", "disabled", "readonly", "required"]
            .into_iter()
            .find(|name| self.directive(name))
        {
            Node::BooleanAttribute(name.to_owned(), expression_value(&self.arguments()?)?)
        } else if let Some(style) = ["class", "style"]
            .into_iter()
            .find(|name| self.directive(name))
        {
            let values = if self.arguments_available {
                expression_value(&self.arguments()?)?
            } else {
                Expression::Value(ViewValue::Null)
            };
            Node::ConditionalAttribute(values, style == "style")
        } else if self.directive("section") {
            let (name, content) = self.named_argument()?;
            match content {
                Some(content) => Node::Section(
                    name,
                    vec![Node::EscapedSectionValue(content)],
                    SectionEnd::Extend,
                ),
                None => {
                    let (body, stop) = self.block(depth + 1)?;
                    match stop {
                        Some(Stop::EndSection) => Node::Section(name, body, SectionEnd::Extend),
                        Some(Stop::Show) => Node::Section(name, body, SectionEnd::Show),
                        Some(Stop::Append) => Node::Section(name, body, SectionEnd::Append),
                        Some(Stop::Overwrite) => Node::Section(name, body, SectionEnd::Overwrite),
                        _ => return Err(invalid("Unclosed Blade section.")),
                    }
                }
            }
        } else {
            return self.custom_directive(depth);
        };
        Ok(Some(node))
    }

    fn statement_start(&self) -> bool {
        self.source.as_bytes()[..self.source.len() - self.remaining.len()]
            .last()
            .is_none_or(|b| !statement_word(*b))
    }

    fn custom_tag(&self) -> Option<(&'a str, &'a str)> {
        if !self.statement_start() {
            return None;
        }
        let rest = self.remaining.strip_prefix('@')?;
        let mut end = rest.bytes().take_while(|b| statement_word(*b)).count();
        if end == 0 {
            return None;
        }
        if let Some(namespace) = rest[end..].strip_prefix("::") {
            let length = namespace.bytes().take_while(|b| statement_word(*b)).count();
            if length > 0 {
                end += 2 + length;
            }
        }
        Some(rest.split_at(end))
    }

    fn custom_directive(&mut self, depth: usize) -> ViewResult<Option<Node>> {
        let Some((name, rest)) = self.custom_tag() else {
            return Ok(None);
        };
        let conditions = &self.extensions.conditions;
        let condition = if conditions.contains_key(name) {
            Some((name, false))
        } else {
            name.strip_prefix("unless")
                .filter(|name| conditions.contains_key(*name))
                .map(|name| (name, true))
        };
        if let Some((name, negated)) = condition {
            self.consume_statement(rest);
            if !self.arguments_available {
                self.consume_close_newline();
            }
            return self.custom_condition(depth, name, negated).map(Some);
        }
        if !self.extensions.directives.contains_key(name) {
            return Ok(None);
        }
        self.consume_statement(rest);
        if !self.arguments_available {
            self.consume_close_newline();
        }
        Ok(Some(Node::CustomDirective(
            name.to_owned(),
            self.custom_arguments()?,
        )))
    }

    fn custom_arguments(&mut self) -> ViewResult<Vec<Expression>> {
        if self.arguments_available {
            expression_arguments(&self.arguments()?)
        } else {
            Ok(Vec::new())
        }
    }

    fn environment_block(
        &mut self,
        depth: usize,
        patterns: Vec<Expression>,
        end: Stop<'a>,
    ) -> ViewResult<Node> {
        let (body, stop) = self.block(depth + 1)?;
        let (alternative, stop) = match stop {
            Some(Stop::Else) => self.block(depth + 1)?,
            _ => (Vec::new(), stop),
        };
        if stop != Some(end) {
            return Err(invalid("Unclosed Blade environment condition."));
        }
        Ok(Node::Environment(patterns, body, alternative))
    }

    fn request_value_block(
        &mut self,
        depth: usize,
        kind: RequestValueKind,
        end: Stop<'a>,
    ) -> ViewResult<Node> {
        let name = expression_value(&self.arguments()?)?;
        let (body, stop) = self.block(depth + 1)?;
        if stop != Some(end) {
            return Err(invalid("Unclosed Blade session or context directive."));
        }
        Ok(Node::RequestValue(kind, name, body))
    }

    fn auth_block(&mut self, depth: usize, mut guest: bool, end: Stop<'a>) -> ViewResult<Node> {
        let mut branches = Vec::new();
        let alternative = loop {
            let guard = if self.arguments_available {
                let raw_arguments = self.arguments()?;
                let guard = raw_arguments.trim();
                (!guard.is_empty())
                    .then(|| expression_value(guard))
                    .transpose()?
            } else {
                None
            };
            let (body, stop) = self.block(depth + 1)?;
            branches.push(AuthBranch { guard, guest, body });
            match stop {
                Some(stop) if stop == end => break Vec::new(),
                Some(Stop::ElseAuth) => guest = false,
                Some(Stop::ElseGuest) => guest = true,
                Some(Stop::Else) => {
                    let (body, stop) = self.block(depth + 1)?;
                    if stop != Some(end) {
                        return Err(invalid("Unclosed Blade authentication block."));
                    }
                    break body;
                }
                _ => return Err(invalid("Unclosed Blade authentication block.")),
            }
        };
        Ok(Node::Auth(branches, alternative))
    }

    fn authorization_block(
        &mut self,
        depth: usize,
        mut mode: AuthorizationMode,
    ) -> ViewResult<Node> {
        let end = Stop::EndAuthorization(mode);
        let mut branches = Vec::new();
        let alternative = loop {
            let mut branch = AuthorizationBranch::parse(mode, &self.arguments()?)?;
            let (body, stop) = self.block(depth + 1)?;
            branch.body = body;
            branches.push(branch);
            match stop {
                Some(stop) if stop == end => break Vec::new(),
                Some(Stop::ElseAuthorization(next)) => mode = next,
                Some(Stop::Else) => {
                    let (body, stop) = self.block(depth + 1)?;
                    if stop != Some(end) {
                        return Err(invalid("Unclosed Blade authorization block."));
                    }
                    break body;
                }
                _ => return Err(invalid("Unclosed Blade authorization block.")),
            }
        };
        Ok(Node::Authorization(branches, alternative))
    }

    fn custom_condition(
        &mut self,
        depth: usize,
        name: &str,
        mut negated: bool,
    ) -> ViewResult<Node> {
        let mut branches = Vec::new();
        let alternative = loop {
            let arguments = self.custom_arguments()?;
            let (body, stop) = self.block(depth + 1)?;
            branches.push(CustomBranch {
                arguments,
                negated,
                body,
            });
            match stop {
                Some(Stop::EndCondition(tag)) if name == tag => break Vec::new(),
                Some(Stop::ElseCondition(tag)) if name == tag => negated = false,
                Some(Stop::Else) => {
                    let (nodes, stop) = self.block(depth + 1)?;
                    if !matches!(stop, Some(Stop::EndCondition(tag)) if name == tag) {
                        return Err(invalid(
                            "Mismatched Blade custom condition closing directive.",
                        ));
                    }
                    break nodes;
                }
                _ => return Err(invalid("Unclosed or mismatched Blade custom condition.")),
            }
        };
        Ok(Node::CustomCondition(
            name.to_owned(),
            branches,
            alternative,
        ))
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

    fn capture_stack_if(&mut self, depth: usize) -> ViewResult<Node> {
        let (condition, name) = self.named_argument()?;
        let name =
            name.ok_or_else(|| invalid("A conditional Blade push requires a stack name."))?;
        let (body, stop) = self.block(depth + 1)?;
        let alternative = match stop {
            Some(Stop::EndPushIf) => Vec::new(),
            Some(Stop::ElsePushIf) => vec![self.capture_stack_if(depth + 1)?],
            _ => return Err(invalid("Unclosed Blade conditional stack capture.")),
        };
        Ok(Node::Conditional(
            ConditionalValue::Expression(condition),
            Predicate::Truthy,
            vec![Node::StackCapture(name, body, false)],
            alternative,
        ))
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

    fn block_ending(&mut self, depth: usize, expected: Stop<'a>) -> ViewResult<Vec<Node>> {
        let (nodes, stop) = self.block(depth + 1)?;
        if stop != Some(expected) {
            return Err(invalid("Unclosed Blade stack or once block."));
        }
        Ok(nodes)
    }

    fn conditional_include(&mut self, when: bool) -> ViewResult<Node> {
        let args = self.arguments()?;
        let separator = argument_separator(&args)
            .ok_or_else(|| invalid("Conditional Blade include requires a view name."))?;
        let condition = expression_value(&args[..separator])?;
        let include = Include::parse(&args[separator + 1..])?;
        Ok(Node::IncludeWhen(condition, include, when))
    }

    fn named_argument(&mut self) -> ViewResult<(Expression, Option<Expression>)> {
        let arguments = self.arguments()?;
        if let Some(index) = argument_separator(&arguments) {
            let name = expression_value(&arguments[..index])?;
            let value = expression_value(&arguments[index + 1..])?;
            Ok((name, Some(value)))
        } else {
            Ok((expression_value(&arguments)?, None))
        }
    }

    fn conditional(
        &mut self,
        depth: usize,
        predicate: Predicate,
        end: Stop<'a>,
    ) -> ViewResult<Node> {
        let expression = if matches!(predicate, Predicate::HasSection | Predicate::SectionMissing) {
            let (name, default) = self.named_argument()?;
            ConditionalValue::Section(name, default)
        } else {
            let arguments = self.arguments()?;
            if let Some(query) = parse_component_query(&arguments)? {
                ConditionalValue::Component(query)
            } else {
                ConditionalValue::Expression(expression_value(&arguments)?)
            }
        };
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

    fn condition(&mut self) -> Option<(Predicate, Stop<'a>)> {
        [
            ("if", Predicate::Truthy, Stop::EndIf),
            ("unless", Predicate::Empty, Stop::EndUnless),
            ("isset", Predicate::Isset, Stop::EndIsset),
            ("empty", Predicate::Empty, Stop::EndEmpty),
            ("hasStack", Predicate::HasStack, Stop::EndIf),
            ("hasSection", Predicate::HasSection, Stop::EndIf),
            ("sectionMissing", Predicate::SectionMissing, Stop::EndIf),
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
        let raw_arguments = self.arguments()?;
        let arguments = raw_arguments.trim();
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

    fn around_loop(&mut self, depth: usize) -> ViewResult<(Vec<Node>, Option<Stop<'a>>)> {
        self.loop_depth += 1;
        let result = self.block(depth + 1);
        self.loop_depth -= 1;
        result
    }

    fn jump(&mut self, kind: JumpKind) -> ViewResult<Node> {
        let (levels, condition) = if self.arguments_available {
            let raw_arguments = self.arguments()?;
            let expression = raw_arguments.trim();
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

    fn switch_block(&mut self, depth: usize) -> ViewResult<Node> {
        let value = expression_value(&self.arguments()?)?;
        let (prefix, mut stop) = self.block(depth + 1)?;
        if prefix
            .iter()
            .any(|node| !matches!(node, Node::Text(text) if text.trim().is_empty()))
        {
            return Err(invalid("The first Blade switch label must follow @switch."));
        }
        let mut seen_default = false;
        let mut cases = Vec::new();
        loop {
            let case_value = match stop {
                Some(Stop::Case) => Some(expression_value(&self.arguments()?)?),
                Some(Stop::Default) if !seen_default => {
                    seen_default = true;
                    None
                }
                Some(Stop::EndSwitch) => break,
                _ => return Err(invalid("Invalid or unclosed Blade switch.")),
            };
            let (body, next) = self.around_loop(depth)?;
            cases.push(SwitchCase {
                value: case_value,
                body,
            });
            stop = next;
        }
        Ok(Node::Switch(value, cases))
    }

    fn stop(&mut self) -> Option<Stop<'a>> {
        if let Some(stop) = self.custom_stop() {
            return Some(stop);
        }
        if let Some((name, rest)) = self.custom_tag()
            && name.eq_ignore_ascii_case("empty")
            && !rest.trim_start_matches([' ', '\t']).starts_with('(')
            && self.directive("empty")
        {
            self.consume_close_newline();
            return Some(Stop::Empty);
        }
        let standard = [
            ("endsection", Stop::EndSection),
            ("stop", Stop::EndSection),
            ("append", Stop::Append),
            ("overwrite", Stop::Overwrite),
            ("show", Stop::Show),
            ("endpush", Stop::EndPush),
            ("endprepend", Stop::EndPrepend),
            ("endonce", Stop::EndOnce),
            ("endPushOnce", Stop::EndPushOnce),
            ("endPrependOnce", Stop::EndPrependOnce),
            ("elsePushIf", Stop::ElsePushIf),
            ("endPushIf", Stop::EndPushIf),
            ("enderror", Stop::EndError),
            ("endfragment", Stop::EndFragment),
            ("endenv", Stop::EndEnvironment),
            ("endproduction", Stop::EndProduction),
            ("endsession", Stop::EndSession),
            ("endcontext", Stop::EndContext),
            ("endauth", Stop::EndAuth),
            ("endguest", Stop::EndGuest),
            ("elseauth", Stop::ElseAuth),
            ("elseguest", Stop::ElseGuest),
            ("endcan", Stop::EndAuthorization(AuthorizationMode::All)),
            (
                "endcannot",
                Stop::EndAuthorization(AuthorizationMode::Denied),
            ),
            ("endcanany", Stop::EndAuthorization(AuthorizationMode::Any)),
            ("elsecan", Stop::ElseAuthorization(AuthorizationMode::All)),
            (
                "elsecannot",
                Stop::ElseAuthorization(AuthorizationMode::Denied),
            ),
            (
                "elsecanany",
                Stop::ElseAuthorization(AuthorizationMode::Any),
            ),
            ("endif", Stop::EndIf),
            ("endunless", Stop::EndUnless),
            ("endforeach", Stop::EndForeach),
            ("endforelse", Stop::EndForelse),
            ("endfor", Stop::EndFor),
            ("endwhile", Stop::EndWhile),
            ("case", Stop::Case),
            ("default", Stop::Default),
            ("endswitch", Stop::EndSwitch),
            ("endisset", Stop::EndIsset),
            ("endempty", Stop::EndEmpty),
            ("elseif", Stop::ElseIf),
            ("else", Stop::Else),
        ]
        .into_iter()
        .find_map(|(name, stop)| self.directive(name).then_some(stop));
        if standard.is_some() {
            return standard;
        }
        None
    }

    fn custom_stop(&mut self) -> Option<Stop<'a>> {
        let (name, rest) = self.custom_tag()?;
        if self.extensions.directives.contains_key(name) {
            return None;
        }
        let (base, is_else) = if let Some(base) = name.strip_prefix("else") {
            (base, true)
        } else {
            (name.strip_prefix("end")?, false)
        };
        if !self.extensions.conditions.contains_key(base) {
            return None;
        }
        self.consume_statement(rest);
        if !is_else && self.arguments_available {
            let _ = self.arguments();
        }
        if !self.arguments_available {
            self.consume_close_newline();
        }
        Some(if is_else {
            Stop::ElseCondition(base)
        } else {
            Stop::EndCondition(base)
        })
    }

    fn directive(&mut self, name: &str) -> bool {
        let Some((token, rest)) = self.custom_tag() else {
            return false;
        };
        if !token.eq_ignore_ascii_case(name)
            || self.extensions.directives.contains_key(token)
            || self.extensions.conditions.contains_key(token)
        {
            return false;
        }
        self.consume_statement(rest);
        let no_arguments = matches!(
            name.to_ascii_lowercase().as_str(),
            "else"
                | "endif"
                | "endunless"
                | "endforeach"
                | "endforelse"
                | "endfor"
                | "endwhile"
                | "endswitch"
                | "default"
                | "endisset"
                | "endempty"
                | "endsection"
                | "stop"
                | "append"
                | "overwrite"
                | "show"
                | "endpush"
                | "endprepend"
                | "endonce"
                | "endpushonce"
                | "endprependonce"
                | "endpushif"
                | "enderror"
                | "endfragment"
                | "endenv"
                | "endproduction"
                | "endsession"
                | "endcontext"
                | "endauth"
                | "endguest"
                | "endcan"
                | "endcannot"
                | "endcanany"
                | "production"
                | "csrf"
        );
        if no_arguments {
            if self.arguments_available {
                // PHP ignores arguments supplied to these compiler methods.
                let _ = self.arguments();
            } else {
                self.consume_close_newline();
            }
        } else if !self.arguments_available
            && matches!(
                name,
                "auth" | "guest" | "once" | "break" | "continue" | "class" | "style"
            )
        {
            self.consume_close_newline();
        }
        true
    }

    fn consume_statement(&mut self, rest: &'a str) {
        self.arguments_available = rest.trim_start_matches([' ', '\t']).starts_with('(');
        self.remaining = rest;
    }

    fn consume_close_newline(&mut self) {
        if let Some(rest) = self
            .remaining
            .strip_prefix("\r\n")
            .or_else(|| self.remaining.strip_prefix('\n'))
            .or_else(|| self.remaining.strip_prefix('\r'))
        {
            self.remaining = rest;
        }
    }

    fn arguments(&mut self) -> ViewResult<Cow<'a, str>> {
        if !self.arguments_available {
            return Err(invalid(
                "Blade directive requires parentheses on the same line.",
            ));
        }
        let rest = self
            .remaining
            .trim_start_matches([' ', '\t'])
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
                    self.consume_close_newline();
                    return Ok(self.restore_raw(&rest[..index]));
                }
                ')' => nested -= 1,
                _ => {}
            }
        }
        Err(invalid("Unclosed Blade expression."))
    }

    fn delimited(&mut self, prefix: usize, delimiter: &str) -> ViewResult<Cow<'a, str>> {
        let (value, rest) = self.remaining[prefix..]
            .split_once(delimiter)
            .ok_or_else(|| invalid("Unclosed Blade expression."))?;
        self.remaining = rest;
        Ok(self.restore_raw(value))
    }

    fn restore_raw(&self, value: &'a str) -> Cow<'a, str> {
        preparation::restore_raw(value, self.raw)
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

fn expression_arguments(input: &str) -> ViewResult<Vec<Expression>> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let mut start = 0;
    separators(input, ",")
        .chain(std::iter::once(input.len()))
        .map(|end| {
            let expression = expression_value(&input[start..end]);
            start = end + 1;
            expression
        })
        .collect()
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
    name.split_once("::").map_or_else(
        || valid_component_path(name),
        |(namespace, path)| valid_component_path(namespace) && valid_component_path(path),
    )
}

fn valid_component_path(name: &str) -> bool {
    name.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
    })
}

fn slot_name(value: &ViewValue) -> ViewResult<String> {
    let name = name_value(value)?;
    if !name.split('-').all(identifier) {
        return Err(render_error("Invalid Blade slot name."));
    }
    Ok(if name.contains('-') {
        Str::camel(name)
    } else {
        name.to_owned()
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
                    && let Some(name) = path.first()
                    && !bindings.iter().any(|(key, _)| *key == name)
                {
                    if path.len() == 1
                        && let Some(slot) = context.slot(name)
                    {
                        output.push_str(slot);
                        continue;
                    }
                    if let Some(attributes) = context.attributes(path) {
                        render_attributes(attributes, output)?;
                        continue;
                    }
                }
                let evaluated = evaluate(expression, data, bindings)?;
                if let Some(text) = evaluated.numeric_text() {
                    output.push_str(&text);
                    continue;
                }
                let evaluated = evaluated.json()?;
                let value = evaluated.as_ref();
                let formatted;
                let value = if matches!(value, ViewValue::Array(_) | ViewValue::Object(_))
                    && let Some(format) = &context.extensions.iterable_echo
                {
                    formatted = ViewValue::String(format(value)?);
                    &formatted
                } else {
                    value
                };
                if *escape
                    && let ViewValue::String(text) = value
                    && let Some(render) = context.escaped_echo
                {
                    render(text, output);
                } else if *escape
                    && context.extensions.preserve_html_entities
                    && let ViewValue::String(text) = value
                {
                    html::escape_preserving_entities(output, text);
                } else {
                    echo(output, value, *escape)?;
                }
            }
            Node::EscapedSectionValue(expression) => {
                echo(output, value(expression, data, bindings)?.as_ref(), true)?;
            }
            Node::Attributes(chain) => {
                render_attributes(&chain.resolve(context, data, bindings)?, output)?;
            }
            Node::ComponentValue(query, escape) => {
                let resolved = query.resolve(context, data, bindings)?;
                if *escape
                    && context.extensions.preserve_html_entities
                    && let ViewValue::String(text) = &resolved
                {
                    html::escape_preserving_entities(output, text);
                } else {
                    echo(output, &resolved, *escape)?;
                }
            }
            Node::Conditional(expression, predicate, yes, no) => {
                let evaluated = match expression {
                    ConditionalValue::Expression(expression) => condition_value(
                        expression,
                        data,
                        bindings,
                        matches!(predicate, Predicate::Isset | Predicate::Empty),
                    )?,
                    ConditionalValue::Component(query) => {
                        Cow::Owned(query.resolve(context, data, bindings)?)
                    }
                    ConditionalValue::Section(expression, default) => {
                        let name = evaluated_name(expression, data, bindings)?;
                        let mut section = String::new();
                        yield_section(
                            context,
                            &name,
                            &mut section,
                            default
                                .as_ref()
                                .map(|default| value(default, data, bindings))
                                .transpose()?
                                .as_deref(),
                        )?;
                        Cow::Owned(ViewValue::String(section))
                    }
                };
                let value = evaluated.as_ref();
                let matches = match predicate {
                    Predicate::Truthy => truthy(value),
                    Predicate::Empty => !truthy(value),
                    Predicate::Isset => !value.is_null(),
                    Predicate::HasStack => context.has_stack(name_value(value)?),
                    Predicate::HasSection => section_present(value),
                    Predicate::SectionMissing => !section_present(value),
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
                let stream = registered_iteration(expression, bindings, context);
                let collection = if stream.is_some() {
                    Cow::Borrowed(&ViewValue::Null)
                } else {
                    value(expression, data, bindings)?
                };
                let (count, entries): (_, Box<dyn Iterator<Item = _>>) = if let Some(stream) =
                    stream
                {
                    let (count, entries) = stream();
                    (
                        count,
                        Box::new(entries.map(|(key, value)| (key, Cow::Owned(value)))),
                    )
                } else {
                    if !collection.is_array() && !collection.is_object() {
                        return Err(render_error(
                            "Blade foreach requires an array, object, or registered iterator.",
                        ));
                    }
                    let count = collection
                        .as_array()
                        .map_or_else(|| collection.as_object().map_or(0, ViewData::len), Vec::len);
                    (
                        Some(count),
                        Box::new(
                            collection_entries(collection.as_ref())
                                .map(|(key, value)| (key, Cow::Borrowed(value))),
                        ),
                    )
                };
                let parent = context.active_loop.clone();
                let depth = parent
                    .as_deref()
                    .and_then(|metadata| metadata["depth"].as_u64())
                    .unwrap_or(0)
                    + 1;
                let mut saw_item = false;
                for (index, (key_value, item_value)) in entries.enumerate() {
                    saw_item = true;
                    let iteration = index + 1;
                    let even = iteration.is_multiple_of(2);
                    let metadata = Arc::new(ViewValue::Object(ViewData::from_iter([
                        ("iteration".into(), iteration.into()),
                        ("index".into(), index.into()),
                        (
                            "remaining".into(),
                            count.map(|count| count.saturating_sub(iteration)).into(),
                        ),
                        ("count".into(), count.into()),
                        ("first".into(), (index == 0).into()),
                        ("last".into(), count.map(|count| iteration == count).into()),
                        ("odd".into(), (!even).into()),
                        ("even".into(), even.into()),
                        ("depth".into(), depth.into()),
                        (
                            "parent".into(),
                            parent.as_deref().cloned().unwrap_or_default(),
                        ),
                    ])));
                    let mut iteration_bindings = bindings.to_vec();
                    if let Some(key) = key {
                        iteration_bindings.push((key, &key_value));
                    }
                    iteration_bindings.push((item, item_value.as_ref()));
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
                if !saw_item {
                    render_block!(alternative, data, bindings, output, context);
                }
            }
            Node::For(counted) => {
                let mut counter = loop_integer(value(&counted.start, data, bindings)?.as_ref())?;
                loop {
                    let current = ViewValue::from(counter);
                    let mut locals = bindings.to_vec();
                    locals.push((&counted.variable, &current));
                    if !boolean_value(&counted.condition, data, &locals)? {
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
                    let step = loop_integer(value(&counted.step, data, &locals)?.as_ref())?;
                    counter = if counted.descending {
                        counter.checked_sub(step)
                    } else {
                        counter.checked_add(step)
                    }
                    .ok_or_else(|| render_error("Blade @for counter overflow."))?;
                }
            }
            Node::While(condition, body) => {
                while boolean_value(condition, data, bindings)? {
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
                if match condition {
                    Some(condition) => boolean_value(condition, data, bindings)?,
                    None => true,
                } {
                    return Ok(Some(Jump {
                        kind: *kind,
                        levels: *levels,
                    }));
                }
            }
            Node::Switch(subject, cases) => {
                let subject = value(subject, data, bindings)?;
                let mut selected = None;
                for (index, case) in cases.iter().enumerate() {
                    if let Some(case) = &case.value
                        && loose_equal(subject.as_ref(), value(case, data, bindings)?.as_ref())
                    {
                        selected = Some(index);
                        break;
                    }
                }
                let selected =
                    selected.or_else(|| cases.iter().position(|case| case.value.is_none()));
                if let Some(selected) = selected {
                    for case in &cases[selected..] {
                        match render_nodes(&case.body, data, bindings, output, context)? {
                            Some(jump) if jump.levels > 1 => return Ok(Some(jump.outer())),
                            Some(_) => break,
                            None => {}
                        }
                    }
                }
            }
            Node::Parent => output.push_str(context.parent()),
            Node::Extends(include) => {
                let name = include.name(data, bindings)?.into_owned();
                context.extend(vec![name], include.data(data, bindings, true)?)?;
            }
            Node::ExtendsFirst(include) => {
                let names = include.names(data, bindings)?;
                context.extend(names, include.data(data, bindings)?)?;
            }
            Node::Section(expression, nodes, end) => {
                let name = evaluated_name(expression, data, bindings)?.into_owned();
                let mut section = String::new();
                render_block!(nodes, data, bindings, &mut section, context);
                match end {
                    SectionEnd::Extend => context.section(name, section),
                    SectionEnd::Show => {
                        context.section(name.clone(), section);
                        context.yield_to(&name, output);
                    }
                    SectionEnd::Append => context.append_section(name, &section),
                    SectionEnd::Overwrite => context.replace_section(name, section),
                }
            }
            Node::Yield(expression, default) => {
                let name = evaluated_name(expression, data, bindings)?;
                yield_section(
                    context,
                    &name,
                    output,
                    default
                        .as_ref()
                        .map(|default| value(default, data, bindings))
                        .transpose()?
                        .as_deref(),
                )?;
            }
            Node::Stack(expression, default) => {
                let name = evaluated_name(expression, data, bindings)?;
                if !context.stack(&name, output)
                    && let Some(default) = default
                {
                    echo(output, value(default, data, bindings)?.as_ref(), false)?;
                }
            }
            Node::StackCapture(expression, nodes, prepend) => {
                let name = evaluated_name(expression, data, bindings)?.into_owned();
                let mut content = String::new();
                render_block!(nodes, data, bindings, &mut content, context);
                context.add_to_stack(name, content, *prepend);
            }
            Node::Once(id, nodes) => {
                let (id, generated) = match id {
                    OnceId::Generated(id) => (Cow::Borrowed(id.as_str()), true),
                    OnceId::Explicit(expression) => {
                        (evaluated_name(expression, data, bindings)?, false)
                    }
                };
                if context.first_render_of(&id, generated) {
                    render_block!(nodes, data, bindings, output, context);
                }
            }
            Node::Csrf => {
                let token = context.request.csrf_token().ok_or_else(|| {
                    render_error("The Blade CSRF field requires an active session.")
                })?;
                hidden_field(output, "_token", &token);
            }
            Node::Method(expression) => {
                hidden_field(
                    output,
                    "_method",
                    &evaluated_name(expression, data, bindings)?,
                );
            }
            Node::Error(field, bag, nodes) => {
                let field = evaluated_name(field, data, bindings)?;
                let mut bag_name = Cow::Borrowed("default");
                if let Some(bag) = bag {
                    bag_name = evaluated_name(bag, data, bindings)?;
                }
                if let Some(message) = data
                    .get("errors")
                    .and_then(|errors| errors.get(bag_name.as_ref()))
                    .and_then(|errors| errors.get(field.as_ref()))
                    .map(|message| message.get(0).unwrap_or(message))
                    .filter(|message| message.as_str().is_some_and(|text| !text.is_empty()))
                {
                    let mut local = bindings.to_vec();
                    local.push(("message", message));
                    render_block!(nodes, data, &local, output, context);
                }
            }
            Node::Fragment(name, nodes) => {
                let name = evaluated_name(name, data, bindings)?.into_owned();
                let mut html = String::new();
                render_block!(nodes, data, bindings, &mut html, context);
                output.push_str(&html);
                context.capture_fragment(name, html);
            }
            Node::BooleanAttribute(name, condition) => {
                if boolean_value(condition, data, bindings)? {
                    output.push_str(name);
                }
            }
            Node::BooleanText(condition) => {
                output.push_str(if boolean_value(condition, data, bindings)? {
                    "true"
                } else {
                    "false"
                });
            }
            Node::JavaScript(expression) => {
                javascript::write_expression(output, value(expression, data, bindings)?.as_ref())?;
            }
            Node::Fonts(aliases) => {
                fonts::render(context, aliases.resolve(data, bindings)?, output)?;
            }
            Node::ConditionalAttribute(entries, style) => {
                let name = if *style { "style" } else { "class" };
                let text = css_list(value(entries, data, bindings)?.as_ref(), *style)?;
                write!(output, "{name}=\"").unwrap();
                escape_html(output, &text);
                output.push('"');
            }
            Node::CustomDirective(name, arguments) => {
                let callback =
                    context.extensions.directives.get(name).ok_or_else(|| {
                        render_error("Blade directive registration was not found.")
                    })?;
                let arguments = arguments
                    .iter()
                    .map(|argument| value(argument, data, bindings).map(Cow::into_owned))
                    .collect::<ViewResult<Vec<_>>>()?;
                match callback(&arguments)? {
                    DirectiveOutput::Text(text) => escape_html(output, &text),
                    DirectiveOutput::Html(html) => output.push_str(&html),
                }
            }
            Node::CustomCondition(name, branches, alternative) => {
                let callback =
                    context.extensions.conditions.get(name).ok_or_else(|| {
                        render_error("Blade condition registration was not found.")
                    })?;
                let mut selected = alternative;
                for branch in branches {
                    let arguments = branch
                        .arguments
                        .iter()
                        .map(|argument| value(argument, data, bindings).map(Cow::into_owned))
                        .collect::<ViewResult<Vec<_>>>()?;
                    if callback(&arguments)? != branch.negated {
                        selected = &branch.body;
                        break;
                    }
                }
                render_block!(selected, data, bindings, output, context);
            }
            Node::Environment(patterns, body, alternative) => {
                let mut names = Vec::new();
                for pattern in patterns {
                    let pattern = value(pattern, data, bindings)?;
                    if let Some(array) = pattern.as_array() {
                        for name in array {
                            names.push(name_value(name)?.to_owned());
                        }
                    } else {
                        names.push(name_value(pattern.as_ref())?.to_owned());
                    }
                }
                let names: Vec<_> = names.iter().map(String::as_str).collect();
                let matching = context.environment_matches(&names)?;
                render_block!(
                    if matching { body } else { alternative },
                    data,
                    bindings,
                    output,
                    context
                );
            }
            Node::Auth(branches, alternative) => {
                let mut selected = alternative;
                for branch in branches {
                    let guard = branch
                        .guard
                        .as_ref()
                        .map(|guard| evaluated_name(guard, data, bindings))
                        .transpose()?;
                    if context.request.authenticated(guard.as_deref())? != branch.guest {
                        selected = &branch.body;
                        break;
                    }
                }
                render_block!(selected, data, bindings, output, context);
            }
            Node::Authorization(branches, alternative) => {
                let mut selected = alternative;
                for branch in branches {
                    if branch.matches(data, bindings, context)? {
                        selected = &branch.body;
                        break;
                    }
                }
                render_block!(selected, data, bindings, output, context);
            }
            Node::RequestValue(kind, name, nodes) => {
                let name = evaluated_name(name, data, bindings)?;
                let contextual = match kind {
                    RequestValueKind::Session => context.request.session(&name),
                    RequestValueKind::Context => context.request.context(&name),
                };
                if let Some(contextual) = contextual {
                    let mut local = bindings.to_vec();
                    local.push(("value", &contextual));
                    render_block!(nodes, data, &local, output, context);
                }
            }
            Node::Include(include, optional) => {
                let name = include.name(data, bindings)?;
                if !optional || context.exists(&name)? {
                    output.push_str(&context.include(&name, &include.data(data, bindings, true)?)?);
                } else {
                    include.evaluate_arguments(data, bindings)?;
                }
            }
            Node::IncludeWhen(condition, include, when) => {
                if boolean_value(condition, data, bindings)? == *when {
                    let name = include.name(data, bindings)?;
                    output.push_str(&context.include(&name, &include.data(data, bindings, true)?)?);
                } else {
                    include.evaluate_arguments(data, bindings)?;
                }
            }
            Node::IncludeFirst(include) => {
                let names = include.names(data, bindings)?;
                let names: Vec<_> = names.iter().map(String::as_str).collect();
                output.push_str(&context.include_first(&names, &include.data(data, bindings)?)?);
            }
            Node::IncludeIsolated(include) => {
                let name = include.name(data, bindings)?;
                output.push_str(
                    &context.include_isolated(&name, &include.data(data, bindings, false)?)?,
                );
            }
            Node::RenderEach(each) => each.render(data, bindings, output, context)?,
            Node::Component {
                name,
                attributes,
                body,
                slots,
            } => {
                let dynamic;
                let name = match name {
                    ComponentName::Static(name) => name.as_str(),
                    ComponentName::Dynamic(expression) => {
                        dynamic = value(expression, data, bindings)?;
                        dynamic
                            .as_str()
                            .filter(|name| valid_component_name(name))
                            .ok_or_else(|| render_error("Invalid dynamic Blade component name."))?
                    }
                };
                let (properties, forwarded) =
                    resolve_tag_attributes(attributes, data, bindings, context)?;
                let properties = Arc::new(properties);
                let component = context.resolve_component(name, &properties)?;
                let scope = match &component {
                    Some(crate::view::component::ResolvedComponent::Hidden) => continue,
                    Some(crate::view::component::ResolvedComponent::Visible { data, .. }) => {
                        Some(ViewValue::Object(data.clone()))
                    }
                    None => None,
                };
                let mut slot_bindings = bindings.to_vec();
                if let Some(scope) = &scope {
                    slot_bindings.push(("component", scope));
                }
                let mut rendered = std::collections::HashMap::new();
                let jump = context.with_component_data(
                    properties.clone(),
                    |context| -> ViewResult<Option<Jump>> {
                        let mut default = String::new();
                        render_block!(body, data, &slot_bindings, &mut default, context);
                        rendered.insert(
                            "slot".to_owned(),
                            RenderSlot {
                                html: default.trim().to_owned(),
                                attributes: ViewData::new(),
                            },
                        );
                        for slot in slots {
                            let name =
                                slot_name(value(&slot.name, data, &slot_bindings)?.as_ref())?;
                            let (values, forwarded) = resolve_tag_attributes(
                                &slot.attributes,
                                data,
                                &slot_bindings,
                                context,
                            )?;
                            let attributes = forwarded.unwrap_or(values);
                            let mut html = String::new();
                            render_block!(&slot.body, data, &slot_bindings, &mut html, context);
                            rendered.insert(
                                name,
                                RenderSlot {
                                    html: html.trim().to_owned(),
                                    attributes,
                                },
                            );
                        }
                        Ok(None)
                    },
                )?;
                if jump.is_some() {
                    return Ok(jump);
                }
                output.push_str(
                    &context.component(name, properties, forwarded, rendered, component)?,
                );
            }
            Node::Slot(_) => return Err(render_error("Blade named slots require a component.")),
        }
    }
    Ok(None)
}

fn take_tag_attribute(attributes: &mut Vec<TagAttribute>, name: &str) -> Option<Expression> {
    let index = attributes
        .iter()
        .position(|attribute| matches!(attribute, TagAttribute::Value(key, _) if key == name))?;
    match attributes.remove(index) {
        TagAttribute::Value(_, expression) => Some(expression),
        TagAttribute::Css(..) | TagAttribute::Forward(..) => unreachable!(),
    }
}

fn resolve_tag_attributes(
    attributes: &[TagAttribute],
    data: &ViewData,
    bindings: &[(&str, &ViewValue)],
    context: &RenderContext<'_>,
) -> ViewResult<(ViewData, Option<ViewData>)> {
    let mut values = ViewData::new();
    let mut parent = None;
    for attribute in attributes {
        let (name, value) = match attribute {
            TagAttribute::Value(name, expression) => (
                name.clone(),
                value(expression, data, bindings)?.into_owned(),
            ),
            TagAttribute::Forward(chain) => {
                parent = Some(chain.resolve(context, data, bindings)?);
                continue;
            }
            TagAttribute::Css(expression, style) => {
                let name = if *style { "style" } else { "class" };
                (
                    name.to_owned(),
                    css_list(value(expression, data, bindings)?.as_ref(), *style)?.into(),
                )
            }
        };
        values.insert(name, value);
    }
    let forwarded = parent.map(|parent| merge_attributes(&parent, values.clone(), &[]));
    Ok((values, forwarded))
}

fn loop_integer(value: &ViewValue) -> ViewResult<i64> {
    value
        .as_i64()
        .ok_or_else(|| render_error("Blade @for requires a signed integer counter and step."))
}

fn yield_section(
    context: &RenderContext<'_>,
    name: &str,
    output: &mut String,
    default: Option<&ViewValue>,
) -> ViewResult<()> {
    if !context.yield_to(name, output)
        && let Some(default) = default
    {
        echo(output, default, true)?;
    }
    Ok(())
}

fn section_present(value: &ViewValue) -> bool {
    let text = value.as_str().unwrap_or_default();
    !matches!(
        text.trim_matches([' ', '\t', '\r', '\n', '\u{000b}', '\0']),
        "" | "0"
    )
}

fn evaluated_name<'a>(
    expression: &'a Expression,
    data: &'a ViewData,
    bindings: &[(&str, &'a ViewValue)],
) -> ViewResult<Cow<'a, str>> {
    match value(expression, data, bindings)? {
        Cow::Borrowed(value) => name_value(value).map(Cow::Borrowed),
        Cow::Owned(value) => name_value(&value).map(|name| Cow::Owned(name.to_owned())),
    }
}

fn name_value(value: &ViewValue) -> ViewResult<&str> {
    value
        .as_str()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| render_error("Blade directive requires a non-empty name."))
}

fn echo(output: &mut String, value: &ViewValue, escape: bool) -> ViewResult<()> {
    match value {
        ViewValue::String(string) if escape => escape_html(output, string),
        ViewValue::String(string) => output.push_str(string),
        ViewValue::Bool(true) => output.push('1'),
        ViewValue::Number(number) => {
            if number.is_f64() {
                output.push_str(&crate::support::input_scalars::double_string(
                    number.as_f64().expect("JSON float"),
                ));
            } else {
                write!(output, "{number}").unwrap();
            }
        }
        ViewValue::Array(_) | ViewValue::Object(_) => {
            return Err(render_error("Array to string conversion in Blade echo."));
        }
        _ => {}
    }
    Ok(())
}

fn hidden_field(output: &mut String, name: &str, value: &str) {
    write!(output, "<input type=\"hidden\" name=\"{name}\" value=\"").unwrap();
    escape_html(output, value);
    output.push_str("\">");
}

fn invalid(message: &'static str) -> ViewError {
    ViewError::CompileFailed(std::io::Error::new(std::io::ErrorKind::InvalidData, message).into())
}

// PHP's non-Unicode PCRE statement grammar uses ASCII word bytes.
fn statement_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

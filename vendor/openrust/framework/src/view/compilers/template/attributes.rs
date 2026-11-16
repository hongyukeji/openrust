use std::{borrow::Cow, fmt::Write as _};

use serde::{Deserialize, Serialize};

use crate::support::Str;

use super::{
    Expression, Node, RenderContext, ViewData, ViewResult, ViewValue, escape_html,
    expression_value, identifier, invalid, render_error, separators, value,
};

pub(super) type Props = Vec<(String, Option<Expression>)>;
pub(super) type AttributeList = Vec<(Expression, Option<Expression>)>;

#[derive(Serialize, Deserialize)]
enum AttributeOperation {
    Merge(Vec<DefaultAttribute>),
    NamedFilter(String),
    Class(AttributeList),
    Style(AttributeList),
    Filter {
        names: Vec<String>,
        prefix: bool,
        include: bool,
    },
}

#[derive(Serialize, Deserialize)]
struct DefaultAttribute {
    name: String,
    value: Expression,
    prepend: bool,
}

#[derive(Serialize, Deserialize)]
pub(super) struct AttributeChain {
    path: Vec<String>,
    operations: Vec<AttributeOperation>,
}

#[derive(Serialize, Deserialize)]
pub(super) enum ComponentQuery {
    Attribute(AttributeChain, AttributeQuery),
    Slot(String, SlotQuery),
}

#[derive(Serialize, Deserialize)]
pub(super) enum AttributeQuery {
    First(Option<Expression>),
    Get(String, Option<Expression>),
    Exists(Vec<String>, bool),
}

#[derive(Serialize, Deserialize)]
pub(super) enum SlotQuery {
    Empty,
    NotEmpty,
    HasActualContent,
}

pub(super) fn array_entries(source: &str) -> ViewResult<Vec<&str>> {
    let source = source
        .trim()
        .strip_prefix('[')
        .and_then(|source| source.strip_suffix(']'))
        .ok_or_else(|| invalid("Expected a Blade array literal."))?;
    Ok(array_items(source))
}

pub(super) fn array_items(source: &str) -> Vec<&str> {
    let mut start = 0;
    let mut entries = Vec::new();
    for end in separators(source, ",") {
        entries.push(source[start..end].trim());
        start = end + 1;
    }
    let last = source[start..].trim();
    if !last.is_empty() {
        entries.push(last);
    }
    entries
}

pub(super) fn array_entry(entry: &str) -> (&str, Option<&str>) {
    match separators(entry, "=>").next() {
        Some(index) => (&entry[..index], Some(&entry[index + 2..])),
        None => (entry, None),
    }
}

pub(super) fn literal_name(input: &str, validate: fn(&str) -> bool) -> ViewResult<String> {
    if let Expression::Value(ViewValue::String(name)) = expression_value(input)?
        && validate(&name)
    {
        return Ok(name);
    }
    Err(invalid("Expected a literal Blade attribute name."))
}

pub(super) fn parse_props(source: &str) -> ViewResult<Props> {
    array_entries(source)?
        .into_iter()
        .map(|entry| {
            let (key, default) = array_entry(entry);
            Ok((
                literal_name(key, identifier)?,
                default.map(expression_value).transpose()?,
            ))
        })
        .collect()
}

pub(super) fn parse_echo(source: &str, escaped: bool) -> ViewResult<Node> {
    let source = source.trim();
    if let Some(argument) = [
        "Js::from(",
        r"Illuminate\Support\Js::from(",
        r"\Illuminate\Support\Js::from(",
    ]
    .into_iter()
    .find_map(|prefix| source.strip_prefix(prefix))
    .and_then(|source| source.strip_suffix(')'))
    {
        return Ok(Node::JavaScript(expression_value(argument)?));
    }
    if let Some(arguments) = source
        .strip_prefix("Vite::fonts(")
        .and_then(|source| source.strip_suffix(')'))
    {
        return super::fonts::parse(arguments);
    }
    if let Some((chain, query)) = parse_chain(source)? {
        return Ok(match query {
            Some(query) => Node::ComponentValue(ComponentQuery::Attribute(chain, query), escaped),
            None => Node::Attributes(chain),
        });
    }
    if let Some(query) = parse_slot_query(source)? {
        return Ok(Node::ComponentValue(query, escaped));
    }
    Ok(Node::Echo(expression_value(source)?, escaped))
}

pub(super) fn parse_component_query(source: &str) -> ViewResult<Option<ComponentQuery>> {
    if let Some((chain, Some(query))) = parse_chain(source)? {
        return Ok(Some(ComponentQuery::Attribute(chain, query)));
    }
    parse_slot_query(source)
}

fn parse_chain(source: &str) -> ViewResult<Option<(AttributeChain, Option<AttributeQuery>)>> {
    let source = source.trim();
    if !source.starts_with('$') {
        return Ok(None);
    }
    let mut separators = separators(source, "->").peekable();
    let mut path = None;
    let mut operations = Vec::new();
    let mut query = None;
    while let Some(index) = separators.next() {
        let end = separators.peek().copied().unwrap_or(source.len());
        let segment = source[index + 2..end].trim();
        if path.is_none() && !segment.contains('(') {
            continue;
        }
        if path.is_none() {
            let Ok(Expression::Variable(target)) = expression_value(&source[..index]) else {
                return Ok(None);
            };
            if !matches!(target.as_slice(), [name] | [_, name] if name == "attributes") {
                return Ok(None);
            }
            path = Some(target);
        }
        let (name, arguments) = segment
            .split_once('(')
            .and_then(|(name, arguments)| Some((name.trim(), arguments.strip_suffix(')')?)))
            .ok_or_else(|| invalid("Expected a Blade attribute bag method call."))?;
        match parse_attribute_query(name, arguments)? {
            Some(access) if separators.peek().is_none() => query = Some(access),
            Some(_) => {
                return Err(invalid(
                    "A Blade attribute lookup must end the method chain.",
                ));
            }
            None => operations.push(parse_operation(name, arguments)?),
        }
    }
    Ok(path.map(|path| (AttributeChain { path, operations }, query)))
}

fn parse_attribute_query(name: &str, arguments: &str) -> ViewResult<Option<AttributeQuery>> {
    let query = match name {
        "first" => AttributeQuery::First(
            (!arguments.trim().is_empty())
                .then(|| expression_value(arguments))
                .transpose()?,
        ),
        "get" => {
            let (key, default) = match separators(arguments, ",").next() {
                Some(index) => (
                    &arguments[..index],
                    Some(expression_value(&arguments[index + 1..])?),
                ),
                None => (arguments, None),
            };
            AttributeQuery::Get(literal_name(key, attribute_name)?, default)
        }
        "has" | "hasAny" => AttributeQuery::Exists(parse_keys(arguments)?, name == "hasAny"),
        _ => return Ok(None),
    };
    Ok(Some(query))
}

fn parse_slot_query(source: &str) -> ViewResult<Option<ComponentQuery>> {
    let Some((target, method)) = source.trim().rsplit_once("->") else {
        return Ok(None);
    };
    let query = match method.trim() {
        "isEmpty()" => SlotQuery::Empty,
        "isNotEmpty()" => SlotQuery::NotEmpty,
        "hasActualContent()" => SlotQuery::HasActualContent,
        _ => return Ok(None),
    };
    match expression_value(target)? {
        Expression::Variable(path) if path.len() == 1 => Ok(Some(ComponentQuery::Slot(
            path.into_iter().next().unwrap(),
            query,
        ))),
        _ => Err(invalid("A Blade slot query requires a slot variable.")),
    }
}

fn parse_keys(arguments: &str) -> ViewResult<Vec<String>> {
    let entries = if arguments.trim_start().starts_with('[') {
        array_entries(arguments)?
    } else {
        vec![arguments]
    };
    entries
        .into_iter()
        .map(|entry| literal_name(entry, |_| true))
        .collect()
}

fn parse_operation(name: &str, arguments: &str) -> ViewResult<AttributeOperation> {
    Ok(match name {
        "merge" => {
            let defaults = array_entries(arguments)?
                .into_iter()
                .map(|entry| {
                    let (key, value) = array_entry(entry);
                    let value =
                        value.ok_or_else(|| invalid("Expected an attribute default value."))?;
                    let method = separators(value, "->")
                        .last()
                        .and_then(|index| value[index + 2..].trim().strip_prefix("prepends("))
                        .and_then(|tail| tail.strip_suffix(')'));
                    Ok(DefaultAttribute {
                        name: literal_name(key, attribute_name)?,
                        value: expression_value(method.unwrap_or(value))?,
                        prepend: method.is_some(),
                    })
                })
                .collect::<ViewResult<_>>()?;
            AttributeOperation::Merge(defaults)
        }
        "class" => AttributeOperation::Class(parse_attribute_list(arguments)?),
        "style" => AttributeOperation::Style(parse_attribute_list(arguments)?),
        "filter" => AttributeOperation::NamedFilter(literal_name(arguments, identifier)?),
        "only" | "except" | "whereStartsWith" | "whereDoesntStartWith" | "thatStartWith" => {
            AttributeOperation::Filter {
                names: parse_keys(arguments)?,
                prefix: !matches!(name, "only" | "except"),
                include: matches!(name, "only" | "whereStartsWith" | "thatStartWith"),
            }
        }
        _ => return Err(invalid("Unknown Blade attribute bag method.")),
    })
}

pub(super) fn parse_attribute_list(source: &str) -> ViewResult<AttributeList> {
    array_entries(source)?
        .into_iter()
        .map(|entry| {
            let (text, condition) = array_entry(entry);
            Ok((
                expression_value(text)?,
                condition.map(expression_value).transpose()?,
            ))
        })
        .collect()
}

pub(super) fn attribute_list(
    entries: &AttributeList,
    data: &ViewData,
    bindings: &[(&str, &ViewValue)],
    style: bool,
) -> ViewResult<String> {
    let mut text = Vec::new();
    for (entry, condition) in entries {
        if condition
            .as_ref()
            .is_some_and(|condition| !super::truthy(value(condition, data, bindings).as_ref()))
        {
            continue;
        }
        let evaluated = value(entry, data, bindings);
        let entry = evaluated
            .as_str()
            .ok_or_else(|| render_error("Blade class and style entries require text."))?
            .trim();
        if !entry.is_empty() {
            text.push(if style {
                format!("{};", entry.trim_end_matches(';'))
            } else {
                entry.to_owned()
            });
        }
    }
    Ok(text.join(" "))
}

pub(super) fn attribute_name(name: &str) -> bool {
    let name = name.strip_prefix(':').unwrap_or(name);
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && characters.all(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '-' | '.' | ':')
        })
}

pub(super) fn apply_props(
    data: &ViewData,
    props: &[(String, Option<Expression>)],
    context: &mut RenderContext<'_>,
) -> ViewData {
    let mut data = data.clone();
    let attributes = context
        .component_attributes
        .get_or_insert_with(ViewData::new);
    for (name, default) in props {
        let kebab = Str::kebab(name);
        let named = attributes.shift_remove(name);
        let alternate = attributes.shift_remove(&kebab);
        if name != &kebab && alternate.is_some() {
            data.shift_remove(&kebab);
        }
        if data.get(name).is_none_or(ViewValue::is_null) {
            let selected = named
                .filter(|value| !value.is_null())
                .or_else(|| alternate.filter(|value| !value.is_null()))
                .or_else(|| {
                    default
                        .as_ref()
                        .map(|default| value(default, &data, &[]).into_owned())
                });
            if let Some(selected) = selected {
                data.insert(name.clone(), selected);
            }
        }
    }
    for name in attributes.keys() {
        data.shift_remove(name);
    }
    data
}

pub(super) fn apply_aware(data: &mut ViewData, props: &Props, context: &RenderContext<'_>) {
    for (name, default) in props {
        let alternate = Str::kebab(name);
        let value = context
            .consumable_property(name, &alternate)
            .cloned()
            .or_else(|| {
                default
                    .as_ref()
                    .map(|default| value(default, data, &[]).into_owned())
            })
            .unwrap_or(ViewValue::Null);
        data.insert(name.clone(), value);
    }
}

impl AttributeChain {
    pub(super) fn resolve(
        &self,
        context: &RenderContext<'_>,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
    ) -> ViewResult<ViewData> {
        let mut bag = context
            .attributes(&self.path)
            .cloned()
            .ok_or_else(|| render_error("A Blade component attribute bag is required."))?;
        for operation in &self.operations {
            bag = operation.apply(bag, context, data, bindings)?;
        }
        Ok(bag)
    }
}

impl ComponentQuery {
    pub(super) fn resolve(
        &self,
        context: &RenderContext<'_>,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
    ) -> ViewResult<ViewValue> {
        match self {
            Self::Slot(name, query) => {
                let html = context.slot(name).unwrap_or_default();
                let matches = match query {
                    SlotQuery::Empty => html.is_empty(),
                    SlotQuery::NotEmpty => !html.is_empty(),
                    SlotQuery::HasActualContent => has_actual_content(html),
                };
                Ok(matches.into())
            }
            Self::Attribute(chain, access) => {
                let attributes = chain.resolve(context, data, bindings)?;
                let with_default = |entry: Option<&ViewValue>, default: &Option<Expression>| {
                    entry
                        .filter(|entry| !entry.is_null())
                        .cloned()
                        .or_else(|| {
                            default
                                .as_ref()
                                .map(|default| value(default, data, bindings).into_owned())
                        })
                        .unwrap_or_default()
                };
                Ok(match access {
                    AttributeQuery::First(default) => {
                        with_default(attributes.values().next(), default)
                    }
                    AttributeQuery::Get(key, default) => with_default(attributes.get(key), default),
                    AttributeQuery::Exists(keys, any) => {
                        let mut present = keys.iter().map(|key| attributes.contains_key(key));
                        if *any {
                            present.any(|found| found)
                        } else {
                            present.all(|found| found)
                        }
                        .into()
                    }
                })
            }
        }
    }
}

fn has_actual_content(mut html: &str) -> bool {
    loop {
        html = html.trim_start();
        let Some(comment) = html.strip_prefix("<!--") else {
            return !html.is_empty();
        };
        let Some(end) = comment.find("-->") else {
            return true;
        };
        html = &comment[end + 3..];
    }
}

impl AttributeOperation {
    fn apply(
        &self,
        mut attributes: ViewData,
        context: &RenderContext<'_>,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
    ) -> ViewResult<ViewData> {
        let mut append = Vec::new();
        let defaults = match self {
            Self::Merge(defaults) => defaults
                .iter()
                .map(|default| {
                    if default.prepend {
                        append.push(default.name.as_str());
                    }
                    (
                        default.name.clone(),
                        value(&default.value, data, bindings).into_owned(),
                    )
                })
                .collect(),
            Self::NamedFilter(name) => {
                let callback = context
                    .extensions
                    .attribute_filters
                    .get(name)
                    .ok_or_else(|| render_error("A Blade attribute filter was not registered."))?;
                attributes.retain(|key, value| callback(key, value));
                return Ok(attributes);
            }
            Self::Class(entries) => ViewData::from_iter([(
                "class".to_owned(),
                attribute_list(entries, data, bindings, false)?.into(),
            )]),
            Self::Style(entries) => ViewData::from_iter([(
                "style".to_owned(),
                attribute_list(entries, data, bindings, true)?.into(),
            )]),
            Self::Filter {
                names,
                prefix,
                include,
            } => {
                attributes.retain(|key, _| {
                    let matches = if *prefix {
                        names
                            .iter()
                            .any(|name| !name.is_empty() && key.starts_with(name))
                    } else {
                        names.contains(key)
                    };
                    matches == *include
                });
                return Ok(attributes);
            }
        };
        Ok(merge_attributes(&attributes, defaults, &append))
    }
}

fn merge_attributes(attributes: &ViewData, mut merged: ViewData, append: &[&str]) -> ViewData {
    for (key, incoming) in attributes {
        let combined = match (key.as_str(), merged.get(key), incoming) {
            (name, Some(ViewValue::String(default)), ViewValue::String(value))
                if matches!(name, "class" | "style") || append.contains(&name) =>
            {
                let (default, value) = (default.trim(), value.trim());
                let text = if default.is_empty() {
                    value.to_owned()
                } else if value.is_empty() {
                    default.to_owned()
                } else if key == "style" {
                    let default = default.trim_end_matches(';');
                    let value = value.trim_end_matches(';');
                    if default == value {
                        format!("{value};")
                    } else {
                        format!("{default}; {value};")
                    }
                } else if default == value {
                    value.to_owned()
                } else {
                    format!("{default} {value}")
                };
                ViewValue::String(text)
            }
            (name, Some(default), ViewValue::Null | ViewValue::Bool(false))
                if matches!(name, "class" | "style") || append.contains(&name) =>
            {
                default.clone()
            }
            _ => incoming.clone(),
        };
        merged.insert(key.clone(), combined);
    }
    merged
}

pub(super) fn render_attributes(attributes: &ViewData, output: &mut String) -> ViewResult<()> {
    let mut separator = "";
    for (key, value) in attributes {
        let text = match value {
            ViewValue::Null | ViewValue::Bool(false) => continue,
            ViewValue::Bool(true) if key == "x-data" || key.starts_with("wire:") => {
                Cow::Borrowed("")
            }
            ViewValue::Bool(true) => Cow::Borrowed(key.as_str()),
            ViewValue::String(text) => Cow::Borrowed(text.trim()),
            ViewValue::Number(number) => Cow::Owned(number.to_string()),
            _ => return Err(render_error("Blade HTML attributes require scalar values.")),
        };
        write!(output, "{separator}{key}=\"").unwrap();
        escape_html(output, &text);
        output.push('"');
        separator = " ";
    }
    Ok(())
}

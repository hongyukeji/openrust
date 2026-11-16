use std::{borrow::Cow, fmt::Write as _};

use crate::support::Str;

use super::{
    Expression, Node, RenderContext, ViewData, ViewResult, ViewValue, escape_html,
    expression_value, identifier, invalid, render_error, separators, value,
};

pub(super) type Props = Vec<(String, Option<Expression>)>;
pub(super) type AttributeList = Vec<(Expression, Option<Expression>)>;

fn array_entries(source: &str) -> ViewResult<Vec<&str>> {
    let source = source
        .trim()
        .strip_prefix('[')
        .and_then(|source| source.strip_suffix(']'))
        .ok_or_else(|| invalid("Expected a Blade attribute array."))?;
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
    Ok(entries)
}

fn array_entry(entry: &str) -> (&str, Option<&str>) {
    match separators(entry, "=>").next() {
        Some(index) => (&entry[..index], Some(&entry[index + 2..])),
        None => (entry, None),
    }
}

fn literal_name(input: &str, validate: fn(&str) -> bool) -> ViewResult<String> {
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
    if let Some(arguments) = source
        .strip_prefix("$attributes->merge(")
        .and_then(|arguments| arguments.strip_suffix(')'))
    {
        let defaults = array_entries(arguments)?
            .into_iter()
            .map(|entry| {
                let (key, value) = array_entry(entry);
                let value = value.ok_or_else(|| invalid("Expected an attribute default value."))?;
                Ok((literal_name(key, attribute_name)?, expression_value(value)?))
            })
            .collect::<ViewResult<_>>()?;
        return Ok(Node::Attributes(defaults));
    }
    Ok(Node::Echo(expression_value(source)?, escaped))
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
            .is_some_and(|condition| !super::truthy(value(condition, data, bindings)))
        {
            continue;
        }
        let entry = value(entry, data, bindings)
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
                        .map(|default| value(default, &data, &[]).clone())
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

pub(super) fn merge_attributes(
    attributes: &ViewData,
    defaults: &[(String, Expression)],
    data: &ViewData,
    bindings: &[(&str, &ViewValue)],
) -> ViewData {
    let mut merged = defaults
        .iter()
        .map(|(key, default)| (key.clone(), value(default, data, bindings).clone()))
        .collect::<ViewData>();
    for (key, incoming) in attributes {
        let combined = match (key.as_str(), merged.get(key), incoming) {
            ("class" | "style", Some(ViewValue::String(default)), ViewValue::String(value))
                if !default.trim().is_empty() && !value.trim().is_empty() =>
            {
                let (default, value) = (default.trim(), value.trim());
                let text = if key == "style" {
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
            ("class" | "style", Some(default), ViewValue::Null | ViewValue::Bool(false)) => {
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

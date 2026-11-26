use std::borrow::Cow;

use iri_string::{
    spec::UriSpec,
    template::{
        UriTemplateStr,
        simple_context::{SimpleContext, Value as Variable},
    },
};
use serde_json::{Map, Value};

use crate::support::Arr;

use super::{Error, Result};

/// the framework joins the base URL and target before expanding URI parameters.
pub(super) fn expand<'a>(
    base: Option<&str>,
    target: &'a str,
    parameters: &Map<String, Value>,
) -> Result<Cow<'a, str>> {
    if parameters.is_empty() {
        return Ok(escape_braces(target));
    }

    let relative = matches!(
        url::Url::parse(target),
        Err(url::ParseError::RelativeUrlWithoutBase)
    );
    let template = if relative && let Some(base) = base {
        Cow::Owned(format!(
            "{}/{}",
            base.trim_end_matches('/'),
            target.trim_start_matches('/')
        ))
    } else {
        Cow::Borrowed(target)
    };

    if !template.contains(['{', '}']) {
        return Ok(template);
    }
    if !template.contains('{') {
        return Ok(Cow::Owned(template.replace('}', "%7D")));
    }

    let template = UriTemplateStr::new(&template).map_err(|_| Error::InvalidUriTemplate)?;
    let mut context = SimpleContext::new();
    for key in template.variables().map(|variable| variable.as_str()) {
        let Some(value) = parameters.get(key) else {
            continue;
        };
        let variable = match value {
            Value::Null => Variable::Undefined,
            Value::Array(values) => {
                Variable::List(values.iter().map(scalar).collect::<Result<_>>()?)
            }
            Value::Object(entries) => Variable::Assoc(associative(entries)?),
            value => Variable::String(scalar(value)?),
        };
        context.insert(key, variable);
    }

    let expanded = template
        .expand::<UriSpec, _>(&context)
        .map_err(|_| Error::InvalidUriTemplate)?;
    Ok(Cow::Owned(expanded.to_string()))
}

pub(super) fn escape_braces(value: &str) -> Cow<'_, str> {
    if value.contains(['{', '}']) {
        Cow::Owned(value.replace('{', "%7B").replace('}', "%7D"))
    } else {
        Cow::Borrowed(value)
    }
}

fn associative(values: &Map<String, Value>) -> Result<Vec<(String, String)>> {
    let mut fields = Vec::new();
    for (key, value) in values {
        if value.is_array() || value.is_object() {
            let nested = Value::Object([(key.clone(), value.clone())].into_iter().collect());
            let query = Arr::query(&nested).map_err(|_| Error::InvalidUriTemplate)?;
            fields.extend(url::form_urlencoded::parse(query.as_bytes()).into_owned());
        } else {
            fields.push((key.clone(), scalar(value)?));
        }
    }
    Ok(fields)
}

fn scalar(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Bool(true) => Ok("1".to_owned()),
        Value::Bool(false) | Value::Null => Ok(String::new()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(Error::InvalidUriTemplate),
    }
}

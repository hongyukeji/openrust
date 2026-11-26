use std::collections::HashMap;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

use super::{RouteParameter, UrlError, UrlResult, UrlValue, url_generator::query_string};

const ROUTE_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'$')
    .add(b'\'')
    .add(b'\x7f')
    .add(b'(')
    .add(b')')
    .add(b'<')
    .add(b'>')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// Generates a URL from a registered route pattern and defaults.
pub struct RouteUrlGenerator {
    path: String,
    domain: Option<String>,
    defaults: HashMap<String, String>,
}

impl RouteUrlGenerator {
    pub(super) fn new(
        path: String,
        domain: Option<String>,
        defaults: HashMap<String, String>,
    ) -> Self {
        Self {
            path,
            domain,
            defaults,
        }
    }

    pub(super) fn to(
        &self,
        root: &::url::Url,
        parameters: &[RouteParameter],
        absolute: bool,
        defaults: &HashMap<String, String>,
        formatter: Option<&super::UrlGenerator>,
    ) -> UrlResult<String> {
        let mut remaining = parameters.to_vec();
        if !defaults.is_empty() {
            self.map_positional_defaults(&mut remaining, defaults);
        }
        let domain = self
            .domain
            .as_deref()
            .map(|domain| self.substitute(domain, &mut remaining, defaults, true))
            .transpose()?;
        let path = self.substitute(&self.path, &mut remaining, defaults, false)?;
        let root_text = if let Some(domain) = domain {
            let mut origin = root.clone();
            origin
                .set_host(Some(&domain))
                .map_err(|_| UrlError::InvalidParameter)?;
            origin.set_path("/");
            origin.as_str().trim_end_matches('/').to_owned()
        } else {
            root.as_str().trim_end_matches('/').to_owned()
        };
        let path = format!("/{}", path.trim_start_matches('/'));
        let output = if let Some(formatter) = formatter {
            formatter.format(&root_text, &path, Some(self))
        } else {
            format!("{root_text}{path}")
        };
        // 框架 先格式化完整 URL，编码后才移除相对 URL 的源和基础路径。
        let mut output = utf8_percent_encode(&output, ROUTE_ENCODE).to_string();
        if !absolute {
            let parsed = ::url::Url::parse(&output).map_err(|_| UrlError::InvalidParameter)?;
            let path = parsed.path();
            let base = root.path().trim_end_matches('/');
            output = if !base.is_empty() && path.starts_with(&format!("{base}/")) {
                path[base.len()..].to_owned()
            } else {
                path.to_owned()
            };
        }
        let query = query_string(&remaining, true)?;
        if !query.is_empty() {
            output.push(if output.contains('?') { '&' } else { '?' });
            output.push_str(&query);
        }
        Ok(output)
    }

    fn map_positional_defaults(
        &self,
        parameters: &mut [RouteParameter],
        defaults: &HashMap<String, String>,
    ) {
        let mut has_default = false;
        let mut vacant = Vec::new();
        let mut required = Vec::new();
        let placeholders = self
            .domain
            .iter()
            .chain(std::iter::once(&self.path))
            .flat_map(|pattern| pattern.split('{').skip(1))
            .filter_map(|tail| tail.split_once('}'));
        for (index, (placeholder, _)) in placeholders.enumerate() {
            let default_key = placeholder.trim_end_matches('?');
            let name = default_key
                .split_once(':')
                .map_or(default_key, |(name, _)| name);
            let has_slot_default = defaults.contains_key(default_key);
            has_default |= has_slot_default;
            if parameters.iter().any(|parameter| {
                matches!(parameter, RouteParameter::Named(key, value) if key == name && !value.empty())
            }) {
                continue;
            }
            vacant.push((index, name));
            if !has_slot_default && !self.defaults.contains_key(name) && !placeholder.ends_with('?')
            {
                required.push(index);
            }
        }
        if !has_default {
            return;
        }
        let count = parameters
            .iter()
            .filter(|item| matches!(item, RouteParameter::Positional(_)))
            .count();
        // the framework assigns an exact positional match to the required slots first.
        // Otherwise it packs supplied values into the available slots from the calculated offset.
        let slots = if count == required.len() {
            vacant
                .into_iter()
                .filter(|(index, _)| required.contains(index))
                .collect::<Vec<_>>()
        } else if required.is_empty() {
            let skip = vacant.len().saturating_sub(count);
            vacant.into_iter().skip(skip).collect()
        } else {
            let start = if count == vacant.len() {
                0
            } else {
                required[0].min(vacant.len().saturating_sub(count))
            };
            vacant
                .into_iter()
                .filter(|(index, _)| *index >= start)
                .collect()
        };
        let values = parameters
            .iter_mut()
            .filter(|item| matches!(item, RouteParameter::Positional(_)));
        for (parameter, (_, name)) in values.zip(slots) {
            let RouteParameter::Positional(value) = parameter else {
                unreachable!()
            };
            *parameter =
                RouteParameter::Named(name.into(), std::mem::replace(value, UrlValue::Null));
        }
    }

    fn substitute(
        &self,
        mut pattern: &str,
        parameters: &mut Vec<RouteParameter>,
        defaults: &HashMap<String, String>,
        host: bool,
    ) -> UrlResult<String> {
        let mut output = String::new();
        while let Some((literal, tail)) = pattern.split_once('{') {
            let (placeholder, rest) = tail.split_once('}').ok_or(UrlError::InvalidParameter)?;
            let (key, optional) = placeholder
                .strip_suffix('?')
                .map_or((placeholder, false), |key| (key, true));
            let default_key = key;
            let (key, field) = key
                .split_once(':')
                .map_or((key, None), |(key, field)| (key, Some(field)));
            let named = parameters.iter().position(|parameter| {
                matches!(parameter, RouteParameter::Named(name, value) if name == key && !value.empty())
            });
            let value = if let Some(index) = named {
                let RouteParameter::Named(_, value) = parameters.remove(index) else {
                    unreachable!()
                };
                Some(value)
            } else if let Some(default) =
                defaults.get(default_key).or_else(|| self.defaults.get(key))
            {
                Some(UrlValue::String(default.clone()))
            } else if let Some(index) = parameters
                .iter()
                .position(|item| matches!(item, RouteParameter::Positional(_)))
            {
                let RouteParameter::Positional(value) = parameters.remove(index) else {
                    unreachable!()
                };
                Some(value)
            } else {
                None
            };
            let value = match (value, field) {
                (Some(UrlValue::Model(model)), Some(field)) => model.route_url_attribute(field)?,
                (Some(UrlValue::Model(model)), None) => model.get_route_key()?,
                (value, _) => value,
            };
            if let Some(value) = value.filter(|value| !value.empty()) {
                let value = value.scalar()?.ok_or(UrlError::InvalidParameter)?;
                if host
                    && (value.is_empty()
                        || !value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                        || value.starts_with('-')
                        || value.ends_with('-'))
                {
                    return Err(UrlError::InvalidParameter);
                }
                output.push_str(literal);
                if host {
                    output.push_str(&value);
                } else {
                    output.push_str(
                        &value
                            .replace('%', "%25")
                            .replace('?', "%3F")
                            .replace('#', "%23"),
                    );
                }
            } else if optional {
                output.push_str(literal.strip_suffix('/').unwrap_or(literal));
            } else {
                return Err(UrlError::MissingParameter);
            }
            pattern = rest;
        }
        output.push_str(pattern);
        Ok(output)
    }
}

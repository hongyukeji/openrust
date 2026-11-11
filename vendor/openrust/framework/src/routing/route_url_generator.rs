use std::collections::HashMap;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

use super::{RouteParameter, UrlError, UrlResult, UrlValue, url_generator::query_string};

const ROUTE_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'$')
    .add(b'\'')
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
    ) -> UrlResult<String> {
        let mut remaining = parameters.to_vec();
        let domain = self
            .domain
            .as_deref()
            .map(|domain| self.substitute(domain, &mut remaining, true))
            .transpose()?;
        let path = self.substitute(&self.path, &mut remaining, false)?;
        let mut output = String::new();
        if absolute {
            if let Some(domain) = domain {
                let mut origin = root.clone();
                origin
                    .set_host(Some(&domain))
                    .map_err(|_| UrlError::InvalidParameter)?;
                output.push_str(origin.as_str().trim_end_matches('/'));
            } else {
                output.push_str(root.as_str().trim_end_matches('/'));
            }
        }
        let path = path.trim_start_matches('/');
        if !absolute || !path.is_empty() {
            output.push('/');
        }
        output.push_str(&utf8_percent_encode(path, ROUTE_ENCODE).to_string());
        let query = query_string(&remaining, true);
        if !query.is_empty() {
            output.push(if output.contains('?') { '&' } else { '?' });
            output.push_str(&query);
        }
        Ok(output)
    }

    fn substitute(
        &self,
        mut pattern: &str,
        parameters: &mut Vec<RouteParameter>,
        host: bool,
    ) -> UrlResult<String> {
        let mut output = String::new();
        while let Some((literal, tail)) = pattern.split_once('{') {
            let (placeholder, rest) = tail.split_once('}').ok_or(UrlError::InvalidParameter)?;
            let (key, optional) = placeholder
                .strip_suffix('?')
                .map_or((placeholder, false), |key| (key, true));
            let key = key.split_once(':').map_or(key, |(key, _)| key);
            let named = parameters.iter().position(|parameter| {
                matches!(parameter, RouteParameter::Named(name, value) if name == key && !value.empty())
            });
            let value = if let Some(index) = named {
                let RouteParameter::Named(_, value) = parameters.remove(index) else {
                    unreachable!()
                };
                Some(value)
            } else if let Some(default) = self.defaults.get(key) {
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
            if let Some(value) = value.filter(|value| !value.empty()) {
                let value = value.scalar().ok_or(UrlError::InvalidParameter)?;
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
                output.push_str(&value);
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

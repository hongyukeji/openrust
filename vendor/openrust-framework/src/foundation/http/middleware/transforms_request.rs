use std::sync::Arc;

use serde_json::Value;

use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response, matches_pattern},
    support::Str,
};

type SkipCallback = Arc<dyn Fn(&Request) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct TrimStrings {
    except: Vec<String>,
    skip: Vec<SkipCallback>,
}

impl Default for TrimStrings {
    fn default() -> Self {
        Self {
            except: ["current_password", "password", "password_confirmation"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            skip: Vec::new(),
        }
    }
}

impl TrimStrings {
    pub fn except(&mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> &mut Self {
        self.except.extend(patterns.into_iter().map(Into::into));
        self
    }

    pub fn skip_when(
        &mut self,
        callback: impl Fn(&Request) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.skip.push(Arc::new(callback));
        self
    }
}

impl Middleware for TrimStrings {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if !self.skip.iter().any(|callback| callback(request)) {
                transform_input(request, |path, value| {
                    if let Value::String(text) = value
                        && !self
                            .except
                            .iter()
                            .any(|pattern| matches_pattern(path, pattern))
                    {
                        let trimmed = Str::trim(text);
                        if trimmed.len() != text.len() {
                            *text = trimmed.to_owned();
                        }
                    }
                })
                .await?;
            }
            Ok(next.run(request).await)
        })
    }
}

#[derive(Clone, Default)]
pub struct ConvertEmptyStringsToNull {
    skip: Vec<SkipCallback>,
}

impl ConvertEmptyStringsToNull {
    pub fn skip_when(
        &mut self,
        callback: impl Fn(&Request) -> bool + Send + Sync + 'static,
    ) -> &mut Self {
        self.skip.push(Arc::new(callback));
        self
    }
}

impl Middleware for ConvertEmptyStringsToNull {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            if !self.skip.iter().any(|callback| callback(request)) {
                transform_input(request, |_, value| {
                    if value.as_str() == Some("") {
                        *value = Value::Null;
                    }
                })
                .await?;
            }
            Ok(next.run(request).await)
        })
    }
}

async fn transform_input(
    request: &mut Request,
    mut transform: impl FnMut(&str, &mut Value),
) -> Result<(), HttpError> {
    let mut remaining = 20 * 1024 * 1024;
    let mut path = String::new();
    request
        .transform_input(|value| clean(value, &mut path, &mut remaining, &mut transform))
        .await
}

fn clean(
    value: &mut Value,
    path: &mut String,
    remaining: &mut usize,
    transform: &mut impl FnMut(&str, &mut Value),
) -> Result<(), HttpError> {
    let children = match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                clean_child(key, child, path, remaining, transform)?;
            }
            return Ok(());
        }
        Value::Array(fields) => fields,
        leaf => {
            transform(path, leaf);
            return Ok(());
        }
    };
    for (index, child) in children.iter_mut().enumerate() {
        clean_child(&index.to_string(), child, path, remaining, transform)?;
    }
    Ok(())
}

fn clean_child(
    key: &str,
    value: &mut Value,
    path: &mut String,
    remaining: &mut usize,
    transform: &mut impl FnMut(&str, &mut Value),
) -> Result<(), HttpError> {
    let previous = path.len();
    *remaining = remaining
        .checked_sub(previous + key.len())
        .ok_or(HttpError::PayloadTooLarge)?;
    path.push_str(key);
    if value.is_array() || value.is_object() {
        path.push('.');
    }
    clean(value, path, remaining, transform)?;
    path.truncate(previous);
    Ok(())
}

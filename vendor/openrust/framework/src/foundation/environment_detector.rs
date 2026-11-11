use std::{ffi::OsString, sync::Arc};

use crate::{
    console::ConsoleError,
    container::{Container, EnvironmentQuery, EnvironmentResult},
    contracts::container::{BindingResolutionError, Container as _},
};

/// Reads the first explicit environment option before the console bootstraps.
pub(super) fn console_environment(arguments: &[OsString]) -> Result<Option<&str>, ConsoleError> {
    let mut arguments = arguments.iter();
    let mut selected = None;
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        }
        let value = if argument == "--env" {
            let value = arguments
                .next()
                .and_then(|value| value.to_str())
                .filter(|value| !value.starts_with('-'))
                .ok_or_else(|| {
                    ConsoleError::new("The --env option requires an environment name.")
                })?;
            Some(value)
        } else if argument.as_encoded_bytes().starts_with(b"--env=") {
            Some(
                argument
                    .to_str()
                    .and_then(|value| value.strip_prefix("--env="))
                    .ok_or_else(|| ConsoleError::new("The --env option requires a UTF-8 name."))?,
            )
        } else {
            None
        };
        if let Some(value) = value {
            if value.is_empty() {
                return Err(ConsoleError::new(
                    "The --env option requires an environment name.",
                ));
            }
            selected.get_or_insert(value);
        }
    }
    Ok(selected)
}

pub(super) fn install(
    container: &Arc<Container>,
    environment: Arc<str>,
) -> Result<(), BindingResolutionError> {
    container.instanceNamed("env", environment)?;
    let weak = Arc::downgrade(container);
    container.resolveEnvironmentUsing(Some(Arc::new(move |patterns| {
        let container = weak
            .upgrade()
            .ok_or_else(|| BindingResolutionError::NotBound {
                service: "env".into(),
            })?;
        query(&container, Some(patterns))
    })));
    Ok(())
}

pub(super) fn query(
    container: &Container,
    patterns: Option<EnvironmentQuery<'_>>,
) -> Result<EnvironmentResult, BindingResolutionError> {
    let current = container.get::<str>("env")?;
    Ok(match patterns {
        None => current.into(),
        Some(EnvironmentQuery::Name(pattern)) => matches_pattern(pattern, &current).into(),
        Some(EnvironmentQuery::Names(patterns)) => patterns
            .iter()
            .any(|pattern| matches_pattern(pattern, &current))
            .into(),
    })
}

// Match literal segments between stars, reserving the final segment for the suffix.
fn matches_pattern(pattern: &str, value: &str) -> bool {
    let Some((prefix, rest)) = pattern.split_once('*') else {
        return pattern == value;
    };
    let Some(value) = value.strip_prefix(prefix) else {
        return false;
    };
    let Some((middle, suffix)) = rest.rsplit_once('*') else {
        return value.ends_with(rest);
    };
    let Some(value) = value.strip_suffix(suffix) else {
        return false;
    };
    middle
        .split('*')
        .try_fold(value, |remaining, part| {
            remaining.split_once(part).map(|(_, tail)| tail)
        })
        .is_some()
}

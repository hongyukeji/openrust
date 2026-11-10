use std::{collections::HashMap, fmt::Write as _};

use regex::{Regex, RegexBuilder};

use super::RoutingError;

const SEPARATORS: &str = "/,;.:-_~+*=@|";

enum Token<'a> {
    Literal(&'a str),
    Parameter {
        name: &'a str,
        separator: &'a str,
        optional: bool,
    },
}

impl Token<'_> {
    fn static_prefix(&self) -> Option<char> {
        match self {
            Self::Literal(text) => text.chars().next(),
            Self::Parameter { separator, .. } => separator.chars().next(),
        }
    }
}

pub(super) struct RoutePattern {
    regex: Regex,
    parameters: Vec<(String, Option<String>)>,
}

impl RoutePattern {
    pub(super) fn compile(
        uri: &str,
        constraints: &HashMap<String, String>,
    ) -> Result<Self, RoutingError> {
        let invalid = || RoutingError::InvalidUri {
            uri: uri.to_owned(),
        };
        let mut parameters = Vec::new();
        let mut tokens = Vec::new();
        let mut remaining = uri;
        while let Some((literal, tail)) = remaining.split_once('{') {
            let (placeholder, rest) = tail.split_once('}').ok_or_else(invalid)?;
            let (placeholder, optional) = placeholder
                .strip_suffix('?')
                .map_or((placeholder, false), |name| (name, true));
            let (name, field) = placeholder
                .split_once(':')
                .map_or((placeholder, None), |(name, field)| (name, Some(field)));
            if !valid_literal(literal)
                || !valid_name(name)
                || field.is_some_and(|field| !valid_name(field))
                || parameters.iter().any(|(previous, _)| *previous == name)
            {
                return Err(invalid());
            }

            let separator_start = literal
                .char_indices()
                .next_back()
                .filter(|(_, ch)| SEPARATORS.contains(*ch))
                .map_or(literal.len(), |(index, _)| index);
            let (literal, separator) = literal.split_at(separator_start);
            if !literal.is_empty() {
                tokens.push(Token::Literal(literal));
            }
            tokens.push(Token::Parameter {
                name,
                separator,
                optional,
            });
            parameters.push((name, field));
            remaining = rest;
        }
        if !valid_literal(remaining) {
            return Err(invalid());
        }
        if !remaining.is_empty() {
            tokens.push(Token::Literal(remaining));
        }

        let optional_start = tokens
            .iter()
            .rposition(|token| !matches!(token, Token::Parameter { optional: true, .. }))
            .map_or(0, |index| index + 1);
        let mut expression = String::from(r"\A");
        for (index, token) in tokens.iter().enumerate() {
            match token {
                Token::Literal(text) => expression.push_str(&regex::escape(text)),
                Token::Parameter {
                    name, separator, ..
                } => {
                    let optional = index >= optional_start;
                    let mut separator = *separator;
                    if index == 0 && optional && separator == "/" {
                        expression.push('/');
                        separator = "";
                    }
                    if optional {
                        expression.push_str("(?:");
                    }

                    let default;
                    let constraint = if let Some(constraint) = constraints.get(*name) {
                        constraint.as_str()
                    } else {
                        let separator = tokens[index + 1..]
                            .iter()
                            .find_map(Token::static_prefix)
                            .filter(|&ch| ch != '/' && SEPARATORS.contains(ch));
                        default = separator.map_or_else(
                            || "[^/]+".to_owned(),
                            |ch| format!("[^/{}]+", regex::escape(&ch.to_string())),
                        );
                        &default
                    };
                    write!(
                        expression,
                        "{}(?P<{name}>{constraint})",
                        regex::escape(separator)
                    )
                    .expect("writing to String");
                }
            }
        }
        expression.push_str(&")?".repeat(tokens.len() - optional_start));
        expression.push_str(r"\z");
        let regex = RegexBuilder::new(&expression)
            .dot_matches_new_line(true)
            .build()
            .map_err(|source| RoutingError::InvalidRoutePattern {
                uri: uri.to_owned(),
                source,
            })?;
        Ok(Self {
            regex,
            parameters: parameters
                .into_iter()
                .map(|(name, field)| (name.to_owned(), field.map(str::to_owned)))
                .collect(),
        })
    }

    pub(super) fn matches(&self, path: &str) -> bool {
        self.regex.is_match(path)
    }

    pub(super) fn binding_fields(&self) -> &[(String, Option<String>)] {
        &self.parameters
    }

    pub(super) fn parameters(
        &self,
        path: &str,
        defaults: &HashMap<String, String>,
    ) -> Vec<(String, String)> {
        let captures = self.regex.captures(path).expect("route already matched");
        let mut remaining_defaults = defaults.clone();
        let mut parameters: Vec<_> = self
            .parameters
            .iter()
            .filter_map(|(name, _)| {
                let default = remaining_defaults.remove(name);
                let value = captures
                    .name(name)
                    .filter(|value| !value.is_empty())
                    .map(|value| value.as_str().to_owned())
                    .or(default)?;
                Some((name.clone(), value))
            })
            .collect();
        parameters.extend(remaining_defaults);
        parameters
    }
}

pub(super) fn constraint(name: &str, expression: &str) -> Result<String, RoutingError> {
    let expression = expression
        .strip_prefix('^')
        .or_else(|| expression.strip_prefix(r"\A"))
        .unwrap_or(expression);
    let expression = expression
        .strip_suffix('$')
        .or_else(|| expression.strip_suffix(r"\z"))
        .unwrap_or(expression);
    if expression.is_empty() || Regex::new(expression).is_err() {
        return Err(RoutingError::InvalidConstraint {
            parameter: name.to_owned(),
            expression: expression.to_owned(),
        });
    }
    Ok(expression.to_owned())
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name != "_fragment"
        && name.len() <= 32
        && bytes
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == b'_')
        && bytes.all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
}

fn valid_literal(text: &str) -> bool {
    !text.contains(['{', '}', '?', '#']) && !text.chars().any(char::is_control)
}

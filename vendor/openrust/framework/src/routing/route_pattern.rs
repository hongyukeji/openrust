use std::{collections::HashMap, fmt::Write as _};

use regex::{Regex, RegexBuilder};

use super::RoutingError;

const SEPARATORS: &str = "/,;.:-_~+*=@|";
const PCRE_LIMITS: &str = "(*LIMIT_MATCH=100000)(*LIMIT_DEPTH=1000)(*LIMIT_HEAP=8192)";
// PCRE2's public compile error codes for unresolved absolute and relative references.
const PCRE_BAD_SUBPATTERN_REFERENCE: i32 = 115;
const PCRE_BAD_RELATIVE_REFERENCE: i32 = 129;

enum Matcher {
    Linear(Regex),
    Pcre(Box<pcre2::bytes::Regex>),
}

fn compile_pcre(expression: &str, host: bool) -> Result<pcre2::bytes::Regex, pcre2::Error> {
    pcre2::bytes::RegexBuilder::new()
        .utf(true)
        .ucp(true)
        .caseless(host)
        .dotall(true)
        .build(&format!("{PCRE_LIMITS}{expression}"))
}

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
    regex: Matcher,
    parameters: Vec<(String, Option<String>)>,
    uri_without_binding_fields: Option<String>,
}

impl RoutePattern {
    pub(super) fn compile(
        uri: &str,
        constraints: &HashMap<String, String>,
    ) -> Result<Self, RoutingError> {
        Self::compile_pattern(uri, constraints, false)
    }

    pub(super) fn compile_host(
        host: &str,
        constraints: &HashMap<String, String>,
    ) -> Result<Self, RoutingError> {
        if host.is_empty()
            || host
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '{' | '}')))
        {
            return Err(RoutingError::InvalidUri {
                uri: host.to_owned(),
            });
        }
        Self::compile_pattern(host, constraints, true)
    }

    fn compile_pattern(
        uri: &str,
        constraints: &HashMap<String, String>,
        host: bool,
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

        let uri_without_binding_fields =
            parameters
                .iter()
                .any(|(_, field)| field.is_some())
                .then(|| {
                    let mut path = String::with_capacity(uri.len());
                    for token in &tokens {
                        match token {
                            Token::Literal(text) => path.push_str(text),
                            Token::Parameter {
                                name,
                                separator,
                                optional,
                            } => {
                                path.push_str(separator);
                                path.push('{');
                                path.push_str(name);
                                if *optional {
                                    path.push('?');
                                }
                                path.push('}');
                            }
                        }
                    }
                    path
                });
        let optional_start = tokens
            .iter()
            .rposition(|token| !matches!(token, Token::Parameter { optional: true, .. }))
            .map_or(0, |index| index + 1);
        let mut expression = String::from(r"\A");
        let mut constrained = false;
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
                        constrained = true;
                        constraint.as_str()
                    } else {
                        let separator = tokens[index + 1..]
                            .iter()
                            .find_map(Token::static_prefix)
                            .filter(|&ch| ch != '/' && SEPARATORS.contains(ch));
                        let excluded = if host { "/." } else { "/" };
                        default = separator.map_or_else(
                            || format!("[^{excluded}]+"),
                            |ch| format!("[^{excluded}{}]+", regex::escape(&ch.to_string())),
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
        let regex = if constrained {
            compile_pcre(&expression, host)
                .map(|regex| Matcher::Pcre(Box::new(regex)))
                .map_err(|source| RoutingError::InvalidPcreRoutePattern {
                    uri: uri.to_owned(),
                    source,
                })?
        } else {
            RegexBuilder::new(&expression)
                .case_insensitive(host)
                .dot_matches_new_line(true)
                .build()
                .map(Matcher::Linear)
                .map_err(|source| RoutingError::InvalidRoutePattern {
                    uri: uri.to_owned(),
                    source,
                })?
        };
        Ok(Self {
            regex,
            uri_without_binding_fields,
            parameters: parameters
                .into_iter()
                .map(|(name, field)| (name.to_owned(), field.map(str::to_owned)))
                .collect(),
        })
    }

    pub(super) fn matches(&self, path: &str) -> bool {
        match &self.regex {
            Matcher::Linear(regex) => regex.is_match(path),
            Matcher::Pcre(regex) => regex.captures(path.as_bytes()).is_ok_and(|captures| {
                captures.is_some_and(|captures| {
                    self.parameters.iter().all(|(name, _)| {
                        captures
                            .name(name)
                            .is_none_or(|value| std::str::from_utf8(value.as_bytes()).is_ok())
                    })
                })
            }),
        }
    }

    pub(super) fn uri_without_binding_fields(&self) -> Option<&str> {
        self.uri_without_binding_fields.as_deref()
    }

    pub(super) fn binding_fields(&self) -> &[(String, Option<String>)] {
        &self.parameters
    }

    pub(super) fn parameters(
        &self,
        path: &str,
        defaults: &HashMap<String, String>,
    ) -> Vec<(String, String)> {
        match &self.regex {
            Matcher::Linear(regex) => {
                let captures = regex.captures(path).expect("route already matched");
                self.parameter_values(defaults, |name| {
                    captures.name(name).map(|value| value.as_str())
                })
            }
            Matcher::Pcre(regex) => {
                let captures = regex
                    .captures(path.as_bytes())
                    .ok()
                    .flatten()
                    .expect("route already matched");
                self.parameter_values(defaults, |name| {
                    captures
                        .name(name)
                        .and_then(|value| std::str::from_utf8(value.as_bytes()).ok())
                })
            }
        }
    }

    fn parameter_values<'a>(
        &self,
        defaults: &HashMap<String, String>,
        mut capture: impl FnMut(&str) -> Option<&'a str>,
    ) -> Vec<(String, String)> {
        let mut remaining_defaults = defaults.clone();
        let mut parameters: Vec<_> = self
            .parameters
            .iter()
            .filter_map(|(name, _)| {
                let default = remaining_defaults.remove(name);
                let value = capture(name)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
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
        .filter(|prefix| trailing_backslashes(prefix).is_multiple_of(2))
        .unwrap_or(expression);
    let normalized = noncapturing_groups(expression);
    // Route variables contribute captures only when the full path or host is compiled.
    let invalid_syntax = compile_pcre(&normalized, false).is_err_and(|error| {
        !matches!(
            error.code(),
            PCRE_BAD_SUBPATTERN_REFERENCE | PCRE_BAD_RELATIVE_REFERENCE
        )
    });
    if expression.is_empty() || invalid_syntax {
        return Err(RoutingError::InvalidConstraint {
            parameter: name.to_owned(),
            expression: expression.to_owned(),
        });
    }
    Ok(normalized)
}

// Symfony removes unnamed captures before inserting the route parameter's own capture.
fn noncapturing_groups(expression: &str) -> String {
    let bytes = expression.as_bytes();
    let mut normalized = String::new();
    let mut copied = 0;
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 1,
            b'(' if cursor + 2 < bytes.len() => {
                cursor += 1;
                if matches!(bytes[cursor], b'?' | b'*') {
                    cursor += 1;
                } else {
                    normalized.push_str(&expression[copied..cursor]);
                    normalized.push_str("?:");
                    copied = cursor;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    normalized.push_str(&expression[copied..]);
    normalized
}

fn trailing_backslashes(prefix: &str) -> usize {
    prefix
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count()
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

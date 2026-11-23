use super::EnvironmentFileError as Error;
use adotenv::{Dialect, Document, DoubleEscapes, QuotedTail, ReferenceRules, UnknownEscapes};
use std::{collections::BTreeMap, ops::Range};
use zeroize::Zeroizing;

fn dialect() -> Dialect {
    Dialect::PORTABLE
        .with_double_escapes(
            DoubleEscapes::LF
                .union(DoubleEscapes::BACKSLASH)
                .union(DoubleEscapes::DOUBLE_QUOTE)
                .union(DoubleEscapes::DOLLAR)
                .union(DoubleEscapes::SINGLE_QUOTE),
        )
        .with_unknown_escapes(UnknownEscapes::Error)
        .with_quoted_tail(QuotedTail::Append)
}
fn parse(text: &str) -> Result<Document<'_>, Error> {
    adotenv::parse_strict_with(text, dialect()).map_err(|_| Error::InvalidDocument)
}
pub(super) fn values(text: &str) -> Result<BTreeMap<String, String>, Error> {
    dotenvy::from_read_iter(text.trim_start_matches('\u{feff}').as_bytes())
        .map(|entry| entry.map_err(|_| Error::InvalidDocument))
        .collect()
}
pub(super) fn validate(text: &str) -> Result<(), Error> {
    if text.len() > 8 * 1024 * 1024 {
        return Err(Error::TooLarge);
    }
    if text.contains('\0') {
        return Err(Error::InvalidDocument);
    }
    parse(text)?;
    values(text)?;
    Ok(())
}
fn key(key: &str) -> Result<(), Error> {
    let mut chars = key.chars();
    if !chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
    {
        return Err(Error::InvalidKey { key: key.into() });
    }
    Ok(())
}
pub(super) fn unique(text: &str, name: &str) -> Result<Option<Range<usize>>, Error> {
    key(name)?;
    let doc = parse(text)?;
    let mut entries = doc.entries().filter(|entry| entry.key() == name);
    let first = entries.next().map(|entry| entry.line_span());
    if entries.next().is_some() {
        return Err(Error::DuplicateKey { key: name.into() });
    }
    Ok(first)
}
pub(super) fn raw(text: &str, name: &str) -> Result<Option<String>, Error> {
    unique(text, name)?;
    let doc = parse(text)?;
    Ok(doc
        .entries()
        .find(|entry| entry.key() == name)
        .and_then(|entry| entry.raw_value().map(str::to_owned)))
}
fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}
fn encode(value: &str, expression: bool) -> String {
    if value.is_empty() {
        return String::new();
    }
    if !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_./:+@%=-,".contains(&c))
    {
        return value.into();
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' if !expression => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
pub(super) fn set(
    text: &str,
    name: &str,
    value: &str,
    expression: bool,
) -> Result<Zeroizing<String>, Error> {
    unique(text, name)?;
    if value.contains('\0') {
        return Err(Error::InvalidValue { key: name.into() });
    }
    let doc = parse(text)?;
    let entry = doc.entries().find(|entry| entry.key() == name);
    if let Some(entry) = entry {
        let expands = entry
            .references()
            .iter()
            .any(|reference| reference.is_eligible(ReferenceRules::PORTABLE));
        if !expression && !expands && values(text)?.get(name).is_some_and(|old| old == value) {
            return Ok(Zeroizing::new(text.into()));
        }
    }
    let encoded = Zeroizing::new(encode(value, expression));
    let mut out = Zeroizing::new(text.to_owned());
    if let Some(entry) = entry {
        let span = entry.value_span().ok_or(Error::InvalidDocument)?;
        out.replace_range(span, &encoded);
    } else {
        let nl = newline(text);
        if !out.trim_start_matches('\u{feff}').is_empty() && !out.ends_with('\n') {
            out.push_str(nl);
        }
        out.push_str(name);
        out.push('=');
        out.push_str(&encoded);
        out.push_str(nl);
    }
    validate(&out)?;
    if !expression && values(&out)?.get(name).map(String::as_str) != Some(value) {
        return Err(Error::InvalidValue { key: name.into() });
    }
    Ok(out)
}
pub(super) fn remove(text: &str, name: &str) -> Result<Zeroizing<String>, Error> {
    unique(text, name)?;
    let doc = parse(text)?;
    let mut out = Zeroizing::new(text.to_owned());
    if let Some(entry) = doc.entries().find(|entry| entry.key() == name) {
        let span = entry.line_span();
        let line = &text[span.clone()];
        let replacement = if let Some(comment) = entry.inline_comment() {
            let indent = &text[entry.tokens().indent.clone()];
            let nl = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            format!("{indent}{comment}{nl}")
        } else {
            String::new()
        };
        out.replace_range(span, &replacement);
    }
    validate(&out)?;
    Ok(out)
}

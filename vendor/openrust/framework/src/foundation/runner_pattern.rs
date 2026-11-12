use std::{fmt::Write as _, iter::Peekable, str::Chars};

use regex::Regex;

use crate::{
    config::{ConfigError, Value},
    contracts::config::Repository,
};

pub(super) struct RunnerFilter(Vec<Regex>);

impl RunnerFilter {
    pub(super) fn from_config(config: &dyn Repository) -> Result<Self, ConfigError> {
        let invalid = || ConfigError::InvalidType {
            key: "app.disabled_runners".into(),
            expected: "an array of runner signature patterns",
        };
        let patterns = match config.get("app.disabled_runners") {
            None => return Ok(Self(Vec::new())),
            Some(Value::Array(patterns)) => patterns,
            _ => return Err(invalid()),
        };
        let mut compiled = Vec::new();
        for pattern in &patterns {
            let pattern = pattern.as_str().ok_or_else(invalid)?;
            if let Some(regex) = compile(pattern) {
                compiled.push(regex);
            } else {
                eprintln!("Invalid disabled runner pattern '{pattern}'");
            }
        }
        Ok(Self(compiled))
    }

    pub(super) fn disables(&self, signature: &str) -> bool {
        self.0.iter().any(|pattern| pattern.is_match(signature))
    }
}

fn compile(pattern: &str) -> Option<Regex> {
    let mut expression = String::from(r"\A");
    let mut characters = pattern.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '*' => {
                while characters.next_if_eq(&'*').is_some() {}
                expression.push_str("[^/]*");
            }
            '?' => expression.push_str("[^/]"),
            '[' => class(&mut expression, &mut characters)?,
            '\\' => literal(&mut expression, characters.next()?),
            character => literal(&mut expression, character),
        }
    }
    expression.push_str(r"\z");
    Regex::new(&expression).ok()
}

fn class(expression: &mut String, characters: &mut Peekable<Chars<'_>>) -> Option<()> {
    let inverse = characters.next_if_eq(&'^').is_some();
    let mut ranges = String::new();
    loop {
        let start = class_character(characters)?;
        if characters.next_if_eq(&'-').is_some() {
            let end = class_character(characters)?;
            if start <= end {
                literal(&mut ranges, start);
                ranges.push('-');
                literal(&mut ranges, end);
            }
        } else {
            literal(&mut ranges, start);
        }
        if characters.next_if_eq(&']').is_some() {
            break;
        }
    }
    if ranges.is_empty() {
        expression.push_str(if inverse { "(?s:.)" } else { r"[^\s\S]" });
    } else {
        expression.push('[');
        if inverse {
            expression.push('^');
        }
        expression.push_str(&ranges);
        expression.push(']');
    }
    Some(())
}

fn class_character(characters: &mut Peekable<Chars<'_>>) -> Option<char> {
    match characters.next()? {
        '\\' => characters.next(),
        '-' | ']' => None,
        character => Some(character),
    }
}

fn literal(expression: &mut String, character: char) {
    write!(expression, r"\x{{{:x}}}", u32::from(character)).expect("writing to a String");
}

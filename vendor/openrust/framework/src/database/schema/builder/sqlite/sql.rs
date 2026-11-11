use crate::database::{DatabaseError, DatabaseResult};

struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

impl Token<'_> {
    fn is(&self, value: &str) -> bool {
        self.text.eq_ignore_ascii_case(value)
    }

    fn identifier(&self) -> String {
        let delimiter = self.text.as_bytes()[0];
        match delimiter {
            b'[' => self.text[1..self.text.len() - 1].to_owned(),
            b'"' | b'`' | b'\'' => {
                let delimiter = char::from(delimiter);
                self.text[1..self.text.len() - 1]
                    .replace(&delimiter.to_string().repeat(2), &delimiter.to_string())
            }
            _ => self.text.to_owned(),
        }
    }
}

fn tokenize(sql: &str) -> DatabaseResult<Vec<Token<'_>>> {
    let bytes = sql.as_bytes();
    let mut index = 0;
    let mut tokens = Vec::new();
    while index < bytes.len() {
        let start = index;
        let byte = bytes[index];
        index += 1;
        match byte {
            byte if byte.is_ascii_whitespace() => continue,
            b'-' if bytes.get(index) == Some(&b'-') => {
                index += bytes[index..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .unwrap_or(bytes.len() - index);
                continue;
            }
            b'/' if bytes.get(index) == Some(&b'*') => {
                index += bytes[index + 1..]
                    .windows(2)
                    .position(|pair| pair == b"*/")
                    .map(|offset| offset + 3)
                    .ok_or(DatabaseError::InvalidSchema("SQLite catalog comment"))?;
                continue;
            }
            b'(' | b')' | b',' | b'.' => {}
            b'"' | b'`' | b'\'' | b'[' => {
                let end = if byte == b'[' { b']' } else { byte };
                loop {
                    let offset = bytes[index..]
                        .iter()
                        .position(|byte| *byte == end)
                        .ok_or(DatabaseError::InvalidSchema("SQLite catalog quotation"))?;
                    index += offset + 1;
                    if end != b']' && bytes.get(index) == Some(&end) {
                        index += 1;
                    } else {
                        break;
                    }
                }
            }
            _ => {
                while bytes.get(index).is_some_and(|byte| {
                    !byte.is_ascii_whitespace()
                        && !matches!(
                            *byte,
                            b'(' | b')' | b',' | b'.' | b'"' | b'`' | b'\'' | b'['
                        )
                }) {
                    index += 1;
                }
            }
        }
        tokens.push(Token {
            text: &sql[start..index],
            start,
            end: index,
        });
    }
    Ok(tokens)
}

pub(super) struct Definition<'a> {
    pub columns: Vec<&'a str>,
    pub constraints: Vec<&'a str>,
    pub suffix: &'a str,
}

pub(super) fn parse_table(sql: &str) -> DatabaseResult<Definition<'_>> {
    let tokens = tokenize(sql)?;
    if tokens.iter().take(3).any(|token| token.is("virtual")) {
        return Err(DatabaseError::UnsupportedSchema(
            "SQLite virtual table rebuild",
        ));
    }
    let mut start = None;
    let mut depth = 0;
    let mut columns = Vec::new();
    let mut constraints = Vec::new();
    for token in &tokens {
        match token.text {
            "(" => {
                if depth == 0 {
                    start = Some(token.end);
                }
                depth += 1;
            }
            ")" | "," if depth == 1 => {
                let start = start.replace(token.end).expect("opened table body");
                let definition = sql[start..token.start].trim();
                let tokens = tokenize(definition)?;
                if constraint_tokens(&tokens).is_some() {
                    constraints.push(definition);
                } else {
                    columns.push(definition);
                }
                if token.text == ")" {
                    return Ok(Definition {
                        columns,
                        constraints,
                        suffix: &sql[token.end..],
                    });
                }
            }
            ")" => depth -= 1,
            _ => {}
        }
    }
    Err(DatabaseError::InvalidSchema(
        "SQLite create table definition",
    ))
}

fn constraint_tokens<'a, 'b>(tokens: &'b [Token<'a>]) -> Option<&'b [Token<'a>]> {
    let body = if tokens.first()?.is("constraint") {
        tokens.get(2..)?
    } else {
        tokens
    };
    let kind = body.first()?;
    ["primary", "foreign", "check", "unique"]
        .iter()
        .any(|name| kind.is(name))
        .then_some(body)
}

pub(super) fn column_name(sql: &str) -> DatabaseResult<String> {
    tokenize(sql)?
        .first()
        .map(Token::identifier)
        .ok_or(DatabaseError::InvalidSchema("SQLite column definition"))
}

pub(super) fn autoincrement_column(sql: &str) -> DatabaseResult<Option<String>> {
    for column in parse_table(sql)?.columns {
        let tokens = tokenize(column)?;
        let mut depth = 0;
        for token in tokens.iter().skip(1) {
            match token.text {
                "(" => depth += 1,
                ")" => depth -= 1,
                _ if depth == 0 && token.is("autoincrement") => {
                    return Ok(tokens.first().map(Token::identifier));
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

pub(super) fn constrained_column(sql: &str) -> DatabaseResult<bool> {
    let tokens = tokenize(sql)?;
    let mut depth = 0;
    for token in tokens.iter().skip(1) {
        match token.text {
            "(" => depth += 1,
            ")" => depth -= 1,
            _ if depth == 0
                && [
                    "primary",
                    "references",
                    "unique",
                    "check",
                    "generated",
                    "as",
                    "collate",
                ]
                .iter()
                .any(|kind| token.is(kind)) =>
            {
                return Ok(true);
            }
            _ => {}
        }
    }
    Ok(false)
}

pub(super) fn primary(sql: &str) -> DatabaseResult<bool> {
    let tokens = tokenize(sql)?;
    Ok(constraint_tokens(&tokens).is_some_and(|tokens| tokens[0].is("primary")))
}

pub(super) fn foreign_matches(
    sql: &str,
    name: Option<&str>,
    columns: &[String],
) -> DatabaseResult<bool> {
    let tokens = tokenize(sql)?;
    let Some(body) = constraint_tokens(&tokens).filter(|body| body[0].is("foreign")) else {
        return Ok(false);
    };
    if let Some(name) = name {
        return Ok(tokens[0].is("constraint") && tokens[1].identifier().eq_ignore_ascii_case(name));
    }
    if body.get(2).is_none_or(|token| !token.is("(")) {
        return Err(DatabaseError::InvalidSchema(
            "SQLite foreign key definition",
        ));
    }
    let actual: Vec<_> = body
        .iter()
        .skip(3)
        .take_while(|token| !token.is(")"))
        .filter(|token| !token.is(","))
        .map(Token::identifier)
        .collect();
    Ok(actual.len() == columns.len()
        && actual
            .iter()
            .zip(columns)
            .all(|(actual, wanted)| actual.eq_ignore_ascii_case(wanted)))
}

pub(super) fn qualify_object(
    sql: &str,
    kind: &str,
    qualified_name: &str,
) -> DatabaseResult<String> {
    let tokens = tokenize(sql)?;
    let index = tokens
        .iter()
        .take(5)
        .position(|token| token.is(kind))
        .ok_or(DatabaseError::InvalidSchema(
            "SQLite catalog object definition",
        ))?;
    let mut name = index + 1;
    if tokens.get(name).is_some_and(|token| token.is("if")) {
        name += 3;
    }
    let start = tokens
        .get(name)
        .ok_or(DatabaseError::InvalidSchema("SQLite catalog object name"))?
        .start;
    if tokens.get(name + 1).is_some_and(|token| token.is(".")) {
        name += 2;
    }
    let end = tokens
        .get(name)
        .ok_or(DatabaseError::InvalidSchema("SQLite catalog object name"))?
        .end;
    Ok(format!("{}{qualified_name}{}", &sql[..start], &sql[end..]))
}

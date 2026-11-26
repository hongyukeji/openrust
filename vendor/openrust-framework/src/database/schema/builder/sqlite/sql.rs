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

pub(super) fn generation_expression(sql: &str) -> DatabaseResult<Option<&str>> {
    let tokens = tokenize(sql)?;
    let mut depth = 0;
    let mut start = None;
    for (index, token) in tokens.iter().enumerate().skip(1) {
        match token.text {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0
                    && let Some(start) = start
                {
                    return Ok(Some(sql[start..token.start].trim()));
                }
            }
            _ if depth == 0 && token.is("as") => {
                start = tokens
                    .get(index + 1)
                    .filter(|next| next.is("("))
                    .map(|next| next.end);
            }
            _ => {}
        }
    }
    Ok(None)
}

pub(super) fn column_collation(sql: &str) -> DatabaseResult<Option<String>> {
    let tokens = tokenize(sql)?;
    let mut depth = 0;
    for (index, token) in tokens.iter().enumerate().skip(1) {
        match token.text {
            "(" => depth += 1,
            ")" => depth -= 1,
            _ if depth == 0 && token.is("collate") => {
                return Ok(tokens
                    .get(index + 1)
                    .map(|name| name.identifier().to_ascii_lowercase()));
            }
            _ => {}
        }
    }
    Ok(None)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InlineKind {
    Primary,
    Unique,
    Check,
    Foreign,
}

pub(super) struct InlineConstraint {
    pub kind: InlineKind,
    pub name: Option<String>,
    pub autoincrement: bool,
    pub range: std::ops::Range<usize>,
}

fn token_is(tokens: &[Token<'_>], index: usize, value: &str) -> bool {
    tokens.get(index).is_some_and(|token| token.is(value))
}

fn parenthesized(tokens: &[Token<'_>], start: usize) -> DatabaseResult<usize> {
    if !token_is(tokens, start, "(") {
        return Err(DatabaseError::InvalidSchema("SQLite constraint expression"));
    }
    let mut depth = 1;
    for (index, token) in tokens.iter().enumerate().skip(start + 1) {
        match token.text {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0 {
                    return Ok(index + 1);
                }
            }
            _ => {}
        }
    }
    Err(DatabaseError::InvalidSchema("SQLite constraint expression"))
}

fn foreign_end(tokens: &[Token<'_>], start: usize) -> DatabaseResult<usize> {
    if tokens.get(start + 1).is_none() {
        return Err(DatabaseError::InvalidSchema(
            "SQLite foreign key definition",
        ));
    }
    let mut end = start + 2;
    if token_is(tokens, end, "(") {
        end = parenthesized(tokens, end)?;
    }
    loop {
        if token_is(tokens, end, "on")
            && (token_is(tokens, end + 1, "delete") || token_is(tokens, end + 1, "update"))
        {
            end += 2;
            end += if token_is(tokens, end, "set") || token_is(tokens, end, "no") {
                2
            } else {
                1
            };
        } else if token_is(tokens, end, "match") {
            end += 2;
        } else {
            break;
        }
    }
    if token_is(tokens, end, "not") && token_is(tokens, end + 1, "deferrable") {
        end += 1;
    }
    if token_is(tokens, end, "deferrable") {
        end += 1;
        if token_is(tokens, end, "initially") {
            end += 2;
        }
    }
    Ok(end)
}

pub(super) fn inline_constraints(sql: &str) -> DatabaseResult<Vec<InlineConstraint>> {
    let tokens = tokenize(sql)?;
    let mut constraints = Vec::new();
    let mut index = 1;
    while let Some(token) = tokens.get(index) {
        if token.is("(") {
            index = parenthesized(&tokens, index)?;
            continue;
        }
        let start = index;
        let name = if token.is("constraint") {
            index += 2;
            Some(
                tokens
                    .get(index - 1)
                    .ok_or(DatabaseError::InvalidSchema(
                        "SQLite inline constraint name",
                    ))?
                    .identifier(),
            )
        } else {
            None
        };
        if token_is(&tokens, index, "default") {
            index += 1;
            if token_is(&tokens, index, "(") {
                index = parenthesized(&tokens, index)?;
            } else {
                index += if token_is(&tokens, index, "+") || token_is(&tokens, index, "-") {
                    2
                } else {
                    1
                };
            }
            continue;
        }
        if token_is(&tokens, index, "collate") {
            index += 2;
            continue;
        }
        let (kind, end) = if token_is(&tokens, index, "primary")
            && token_is(&tokens, index + 1, "key")
        {
            let mut end = index + 2;
            if token_is(&tokens, end, "asc") || token_is(&tokens, end, "desc") {
                end += 1;
            }
            if token_is(&tokens, end, "on") && token_is(&tokens, end + 1, "conflict") {
                end += 3;
            }
            if token_is(&tokens, end, "autoincrement") {
                end += 1;
            }
            (InlineKind::Primary, end)
        } else if token_is(&tokens, index, "unique") {
            let end =
                if token_is(&tokens, index + 1, "on") && token_is(&tokens, index + 2, "conflict") {
                    index + 4
                } else {
                    index + 1
                };
            (InlineKind::Unique, end)
        } else if token_is(&tokens, index, "check") && token_is(&tokens, index + 1, "(") {
            (InlineKind::Check, parenthesized(&tokens, index + 1)?)
        } else if token_is(&tokens, index, "references") {
            (InlineKind::Foreign, foreign_end(&tokens, index)?)
        } else {
            index += 1;
            continue;
        };
        let last = tokens
            .get(end - 1)
            .ok_or(DatabaseError::InvalidSchema("SQLite inline constraint"))?;
        constraints.push(InlineConstraint {
            kind,
            name,
            autoincrement: kind == InlineKind::Primary && last.is("autoincrement"),
            range: tokens[start].start..last.end,
        });
        index = end;
    }
    Ok(constraints)
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

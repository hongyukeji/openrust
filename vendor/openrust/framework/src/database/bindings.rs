use std::{borrow::Cow, collections::BTreeMap};

use sqlx::Either;

use super::{DatabaseError, DatabaseResult, Value};

/// Parameters for a prepared SQL statement.
#[derive(Clone, Copy, Debug)]
pub enum Bindings<'a> {
    /// Values in the order of their positional placeholders.
    Positional(&'a [Value]),
    /// Values indexed by the names of their `:name` placeholders.
    Named(&'a [(&'a str, Value)]),
}

impl<'a> From<&'a [Value]> for Bindings<'a> {
    fn from(values: &'a [Value]) -> Self {
        Self::Positional(values)
    }
}

impl<'a, const N: usize> From<&'a [Value; N]> for Bindings<'a> {
    fn from(values: &'a [Value; N]) -> Self {
        Self::Positional(values)
    }
}

impl<'a> From<&'a Vec<Value>> for Bindings<'a> {
    fn from(values: &'a Vec<Value>) -> Self {
        Self::Positional(values)
    }
}

impl<'a> Bindings<'a> {
    pub(super) fn prepare(self, sql: &'a str, mysql: bool) -> DatabaseResult<PreparedBindings<'a>> {
        match self {
            Self::Positional(values) => Ok(PreparedBindings {
                sql: Cow::Borrowed(sql),
                values: Either::Left(values),
            }),
            Self::Named(values) => prepare_named(sql, values, mysql),
        }
    }
}

pub(super) struct PreparedBindings<'a> {
    pub(super) sql: Cow<'a, str>,
    values: Either<&'a [Value], Vec<&'a Value>>,
}

impl<'a> PreparedBindings<'a> {
    pub(super) fn values(&self) -> impl ExactSizeIterator<Item = &'a Value> + '_ {
        match &self.values {
            Either::Left(values) => Either::Left(values.iter()),
            Either::Right(values) => Either::Right(values.iter().copied()),
        }
    }
}

fn invalid(reason: &'static str) -> DatabaseError {
    DatabaseError::InvalidNamedBindings(reason)
}

fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn prepare_named<'a>(
    sql: &str,
    bindings: &'a [(&str, Value)],
    mysql: bool,
) -> DatabaseResult<PreparedBindings<'a>> {
    let mut names = BTreeMap::new();
    for (key, value) in bindings {
        let name = key.strip_prefix(':').unwrap_or(key);
        if name.is_empty() || !name.bytes().all(name_byte) {
            return Err(invalid("invalid parameter name"));
        }
        if names.insert(name, (value, false)).is_some() {
            return Err(invalid("duplicate binding key"));
        }
    }

    let mut query = String::with_capacity(sql.len());
    let mut values = Vec::with_capacity(bindings.len());
    let mut copied = 0;
    let mut offset = 0;
    let bytes = sql.as_bytes();
    while let Some(&byte) = bytes.get(offset) {
        let tail = &bytes[offset..];
        if tail.starts_with(b"/*") {
            offset += tail[2..]
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(tail.len(), |end| end + 4);
        } else if (mysql && byte == b'#')
            || (tail.starts_with(b"--")
                && (!mysql || tail.get(2).is_none_or(|b| *b <= b' ' || *b == 127)))
        {
            offset += tail.iter().position(|b| *b == b'\n').unwrap_or(tail.len());
        } else if matches!(byte, b'\'' | b'"' | b'`') || (!mysql && byte == b'[') {
            let delimiter = if byte == b'[' { b']' } else { byte };
            offset += quoted_len(tail, delimiter, mysql && byte != b'`');
        } else if tail.starts_with(b"::") {
            offset += tail.iter().take_while(|&&b| b == b':').count();
        } else if byte == b':' && tail.get(1).is_some_and(|b| name_byte(*b)) {
            let end = offset + 1 + tail[1..].iter().take_while(|&&b| name_byte(b)).count();
            let name = &sql[offset + 1..end];
            let (value, used) = names
                .get_mut(name)
                .ok_or_else(|| invalid("missing binding key"))?;
            if mysql && *used {
                return Err(invalid("repeated placeholder on MySQL"));
            }
            *used = true;
            values.push(*value);
            query.push_str(&sql[copied..offset]);
            query.push('?');
            copied = end;
            offset = end;
        } else if byte == b'?' {
            return Err(invalid("cannot mix named and positional placeholders"));
        } else {
            offset += 1;
        }
    }
    if names.values().any(|(_, used)| !used) {
        return Err(invalid("unused binding key"));
    }
    query.push_str(&sql[copied..]);
    Ok(PreparedBindings {
        sql: query.into(),
        values: Either::Right(values),
    })
}

fn quoted_len(bytes: &[u8], delimiter: u8, backslash_escape: bool) -> usize {
    let mut offset = 1;
    while let Some(&byte) = bytes.get(offset) {
        offset += 1;
        if backslash_escape && byte == b'\\' && offset < bytes.len() {
            offset += 1;
        } else if byte == delimiter {
            if delimiter == b']' || bytes.get(offset) != Some(&delimiter) {
                break;
            }
            offset += 1;
        }
    }
    offset
}

use std::{borrow::Cow, collections::BTreeMap, fmt::Write as _};

use sqlx::Either;

use super::{DatabaseError, DatabaseResult, Value, events::QueryBindings};

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
            Self::Named(values) => prepare_named(
                sql,
                values.iter().map(|(key, value)| (*key, value)),
                mysql,
                false,
            ),
        }
    }

    pub(super) fn prepare_postgres(self, sql: &'a str) -> DatabaseResult<PreparedBindings<'a>> {
        match self {
            Self::Positional(_) => self.prepare(sql, false).map(PreparedBindings::postgres),
            Self::Named(values) => prepare_named(
                sql,
                values.iter().map(|(key, value)| (*key, value)),
                false,
                true,
            )
            .map(PreparedBindings::postgres),
        }
    }
}

pub(super) struct PreparedBindings<'a> {
    pub(super) sql: Cow<'a, str>,
    values: Either<&'a [Value], Vec<&'a Value>>,
}

impl<'a> PreparedBindings<'a> {
    pub(super) fn postgres(self) -> Self {
        self.numbered("$", true)
    }

    pub(super) fn sql_server(self) -> Self {
        self.numbered("@P", false)
    }

    fn numbered(mut self, prefix: &str, postgres: bool) -> Self {
        if !self.sql.contains('?') || self.values().len() == 0 {
            return self;
        }
        let bytes = self.sql.as_bytes();
        let mut sql = String::with_capacity(bytes.len());
        let mut start = 0;
        let mut pos = 0;
        let mut index = 0;
        while let Some(&byte) = bytes.get(pos) {
            let tail = &bytes[pos..];
            let skipped = if postgres {
                skip_postgres(tail)
            } else {
                skip_sql(tail, false)
            };
            if let Some(len) = skipped {
                pos += len;
            } else if byte == b'?' {
                sql.push_str(&self.sql[start..pos]);
                if tail.starts_with(b"??") {
                    sql.push('?');
                    pos += 2;
                } else {
                    index += 1;
                    write!(sql, "{prefix}{index}").expect("writing to a String");
                    pos += 1;
                }
                start = pos;
            } else {
                pos += 1;
            }
        }
        sql.push_str(&self.sql[start..]);
        self.sql = Cow::Owned(sql);
        self
    }

    pub(super) fn values(&self) -> impl ExactSizeIterator<Item = &'a Value> + '_ {
        match &self.values {
            Either::Left(values) => Either::Left(values.iter()),
            Either::Right(values) => Either::Right(values.iter().copied()),
        }
    }

    pub(super) fn emulate_postgres(&self) -> DatabaseResult<String> {
        let values: Vec<_> = self.values().collect();
        let mut seen = vec![false; values.len()];
        let mut sql = String::with_capacity(self.sql.len());
        let mut start = 0;
        let mut offset = 0;
        let bytes = self.sql.as_bytes();
        while let Some(tail) = bytes.get(offset..).filter(|tail| !tail.is_empty()) {
            if let Some(len) = skip_postgres(tail) {
                offset += len;
            } else if tail[0] == b'$' && tail.get(1).is_some_and(u8::is_ascii_digit) {
                let end = offset
                    + 1
                    + tail[1..]
                        .iter()
                        .take_while(|byte| byte.is_ascii_digit())
                        .count();
                let parameter = self.sql[offset + 1..end]
                    .parse::<usize>()
                    .map_err(|_| DatabaseError::InvalidQuery("PostgreSQL parameter index"))?;
                let index = parameter
                    .checked_sub(1)
                    .filter(|&index| index < values.len())
                    .ok_or(DatabaseError::InvalidBindings {
                        expected: parameter,
                        actual: values.len(),
                    })?;
                sql.push_str(&self.sql[start..offset]);
                postgres_literal(&mut sql, values[index])?;
                seen[index] = true;
                start = end;
                offset = end;
            } else {
                offset += 1;
            }
        }
        let expected = seen.into_iter().filter(|used| *used).count();
        if expected != values.len() {
            return Err(DatabaseError::InvalidBindings {
                expected,
                actual: values.len(),
            });
        }
        sql.push_str(&self.sql[start..]);
        Ok(sql)
    }
}

fn postgres_literal(sql: &mut String, value: &Value) -> DatabaseResult<()> {
    fn string_literal(sql: &mut String, value: &str) -> DatabaseResult<()> {
        if value.contains('\0') {
            return Err(DatabaseError::InvalidQuery(
                "PostgreSQL text parameter contains NUL",
            ));
        }
        // Encode the payload without SQL delimiters, independent of session escaping settings.
        sql.push_str("E'");
        for byte in value.bytes() {
            write!(sql, "\\x{byte:02x}").expect("writing to a String");
        }
        sql.push('\'');
        Ok(())
    }
    match value {
        Value::Null => sql.push_str("NULL"),
        Value::Bool(value) => sql.push_str(if *value { "TRUE" } else { "FALSE" }),
        Value::Integer(value) => write!(sql, "({value})").expect("writing to a String"),
        Value::Unsigned(value) => {
            let value = i64::try_from(*value)
                .map_err(|_| DatabaseError::InvalidQuery("PostgreSQL integer parameter"))?;
            write!(sql, "({value})").expect("writing to a String");
        }
        Value::Real(value) => write!(sql, "('{value}'::float8)").expect("writing to a String"),
        Value::Text(value) => string_literal(sql, value)?,
        Value::Bytes(value) => {
            sql.push_str("(E'\\\\x");
            for byte in value {
                write!(sql, "{byte:02x}").expect("writing to a String");
            }
            sql.push_str("'::bytea)");
        }
        Value::Json(value) => {
            sql.push('(');
            string_literal(sql, &value.to_string())?;
            sql.push_str("::jsonb)");
        }
    }
    Ok(())
}

fn skip_postgres(tail: &[u8]) -> Option<usize> {
    let first = *tail.first()?;
    if tail.starts_with(b"/*") {
        let mut offset = 2;
        let mut depth = 1;
        while let Some(pair) = tail.get(offset..offset + 2) {
            match pair {
                b"/*" => {
                    depth += 1;
                    offset += 2;
                }
                b"*/" => {
                    depth -= 1;
                    offset += 2;
                    if depth == 0 {
                        return Some(offset);
                    }
                }
                _ => offset += 1,
            }
        }
        return Some(tail.len());
    }
    if matches!(first, b'e' | b'E') && tail.get(1) == Some(&b'\'') {
        return Some(1 + quoted_len(&tail[1..], b'\'', true));
    }
    if matches!(first, b'\'' | b'"') {
        return Some(quoted_len(tail, first, false));
    }
    if tail.starts_with(b"--") {
        return Some(
            tail.iter()
                .position(|byte| matches!(byte, b'\r' | b'\n'))
                .unwrap_or(tail.len()),
        );
    }
    if first.is_ascii_alphabetic() || first == b'_' || !first.is_ascii() {
        return Some(
            tail.iter()
                .take_while(|&&byte| name_byte(byte) || byte == b'$' || !byte.is_ascii())
                .count(),
        );
    }
    postgres_dollar_quote(tail)
}

fn postgres_dollar_quote(tail: &[u8]) -> Option<usize> {
    let rest = tail.strip_prefix(b"$")?;
    if rest.first()?.is_ascii_digit() {
        return None;
    }
    let tag = rest
        .iter()
        .take_while(|byte| name_byte(**byte) || !byte.is_ascii())
        .count()
        + 1;
    if tail.get(tag) != Some(&b'$') {
        return None;
    }
    let delimiter = &tail[..=tag];
    let body = &tail[tag + 1..];
    Some(
        body.windows(delimiter.len())
            .position(|part| part == delimiter)
            .map_or(tail.len(), |end| end + delimiter.len() * 2),
    )
}

fn invalid(reason: &'static str) -> DatabaseError {
    DatabaseError::InvalidNamedBindings(reason)
}

fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn prepare_named<'a>(
    sql: &str,
    bindings: impl ExactSizeIterator<Item = (&'a str, &'a Value)>,
    mysql: bool,
    postgres: bool,
) -> DatabaseResult<PreparedBindings<'a>> {
    let capacity = bindings.len();
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
    let mut values = Vec::with_capacity(capacity);
    let mut copied = 0;
    let mut offset = 0;
    let bytes = sql.as_bytes();
    while let Some(&byte) = bytes.get(offset) {
        let tail = &bytes[offset..];
        let skipped = if postgres {
            skip_postgres(tail)
        } else {
            skip_sql(tail, mysql)
        };
        if let Some(length) = skipped {
            offset += length;
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

fn skip_sql(tail: &[u8], mysql: bool) -> Option<usize> {
    let byte = *tail.first()?;
    if tail.starts_with(b"/*") {
        Some(
            tail[2..]
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(tail.len(), |end| end + 4),
        )
    } else if (mysql && byte == b'#')
        || (tail.starts_with(b"--")
            && (!mysql || tail.get(2).is_none_or(|b| *b <= b' ' || *b == 127)))
    {
        Some(tail.iter().position(|b| *b == b'\n').unwrap_or(tail.len()))
    } else if matches!(byte, b'\'' | b'"' | b'`') || (!mysql && byte == b'[') {
        let delimiter = if byte == b'[' { b']' } else { byte };
        Some(quoted_len(tail, delimiter, mysql && byte != b'`'))
    } else {
        None
    }
}

fn quoted_len(bytes: &[u8], delimiter: u8, backslash_escape: bool) -> usize {
    let mut offset = 1;
    while let Some(&byte) = bytes.get(offset) {
        offset += 1;
        if backslash_escape && byte == b'\\' && offset < bytes.len() {
            offset += 1;
        } else if byte == delimiter {
            if bytes.get(offset) != Some(&delimiter) {
                break;
            }
            offset += 1;
        }
    }
    offset
}

pub(super) fn render_query(
    sql: &str,
    bindings: &QueryBindings,
    mysql: bool,
) -> DatabaseResult<String> {
    let prepared = match bindings {
        QueryBindings::Positional(values) => Bindings::Positional(values).prepare(sql, mysql)?,
        QueryBindings::Named(values) => prepare_named(
            sql,
            values.iter().map(|(key, value)| (key.as_str(), value)),
            mysql,
            false,
        )?,
    };
    let bindings = prepared
        .values()
        .map(|value| escape(value, mysql))
        .collect::<DatabaseResult<Vec<_>>>()?;
    if bindings.is_empty() {
        return Ok(prepared.sql.into_owned());
    }

    let mut rendered = String::with_capacity(prepared.sql.len());
    let mut tokens = Vec::new();
    let mut slots = BTreeMap::new();
    let mut named_slots = BTreeMap::new();
    let mut max_slot = 0usize;
    let mut argument = 0usize;
    let mut copied = 0;
    let bytes = prepared.sql.as_bytes();
    let mut offset = 0;
    while let Some(&byte) = bytes.get(offset) {
        let tail = &bytes[offset..];
        if let Some(length) = skip_sql(tail, mysql) {
            offset += length;
            continue;
        }
        if !mysql && byte == b';' {
            append_sqlite_parameters(
                &prepared.sql,
                &bindings,
                &tokens,
                &slots,
                &mut argument,
                &mut copied,
                &mut rendered,
            );
            tokens.clear();
            slots.clear();
            named_slots.clear();
            max_slot = 0;
        } else if byte == b'?'
            || (!mysql && byte == b'$' && tail.get(1).is_some_and(u8::is_ascii_digit))
        {
            if mysql {
                if tail.starts_with(b"??") {
                    offset += 2;
                    continue;
                }
                if let Some(value) = bindings.get(argument) {
                    rendered.push_str(&prepared.sql[copied..offset]);
                    rendered.push_str(value);
                    copied = offset + 1;
                    argument += 1;
                }
            } else {
                let digits = tail[1..].iter().take_while(|b| b.is_ascii_digit()).count();
                let end = offset + 1 + digits;
                let token = &prepared.sql[offset..end];
                let number = (digits != 0)
                    .then(|| token[1..].parse::<usize>())
                    .transpose()
                    .map_err(|_| DatabaseError::InvalidQuery("numbered placeholder"))?;
                let slot = match byte {
                    b'$' => *named_slots
                        .entry(token)
                        .or_insert_with(|| max_slot.saturating_add(1)),
                    _ => number.unwrap_or_else(|| max_slot.saturating_add(1)),
                };
                if slot == 0 || number == Some(0) {
                    return Err(DatabaseError::InvalidQuery("numbered placeholder"));
                }
                max_slot = max_slot.max(slot);
                let entry = slots.entry(slot).or_insert(None);
                if entry.is_none() {
                    *entry = number;
                }
                tokens.push((offset, end, slot));
                offset = end;
                continue;
            }
        }
        offset += 1;
    }
    if !mysql {
        append_sqlite_parameters(
            &prepared.sql,
            &bindings,
            &tokens,
            &slots,
            &mut argument,
            &mut copied,
            &mut rendered,
        );
    }
    rendered.push_str(&prepared.sql[copied..]);
    Ok(rendered)
}

fn append_sqlite_parameters(
    sql: &str,
    bindings: &[String],
    tokens: &[(usize, usize, usize)],
    slots: &BTreeMap<usize, Option<usize>>,
    argument: &mut usize,
    copied: &mut usize,
    output: &mut String,
) {
    let mut values = BTreeMap::new();
    let mut previous_slot = 0;
    for (&slot, &number) in slots {
        // SQLx advances its anonymous-argument offset across gaps in SQLite's slot table.
        *argument = argument.saturating_add(slot - previous_slot - 1);
        let index = number.unwrap_or_else(|| {
            *argument = argument.saturating_add(1);
            *argument
        });
        let Some(value) = index.checked_sub(1).and_then(|index| bindings.get(index)) else {
            break;
        };
        values.insert(slot, value.as_str());
        previous_slot = slot;
    }
    for &(start, end, slot) in tokens {
        output.push_str(&sql[*copied..start]);
        output.push_str(values.get(&slot).copied().unwrap_or("null"));
        *copied = end;
    }
}

fn escape(value: &Value, mysql: bool) -> DatabaseResult<String> {
    use std::fmt::Write as _;
    fn string(value: &str, mysql: bool) -> DatabaseResult<String> {
        if value.contains('\0') {
            return Err(DatabaseError::InvalidQuery(
                "text binding contains a null byte",
            ));
        }
        let value = value.replace('\'', "''");
        let value = if mysql {
            value.replace('\\', "\\\\")
        } else {
            value
        };
        Ok(format!("'{value}'"))
    }

    Ok(match value {
        Value::Null => "null".into(),
        Value::Bool(value) => if *value { "1" } else { "0" }.into(),
        Value::Integer(value) => value.to_string(),
        Value::Unsigned(value) => value.to_string(),
        Value::Real(value) if value.is_finite() => value.to_string(),
        Value::Real(_) => return Err(DatabaseError::InvalidQuery("non-finite numeric binding")),
        Value::Text(value) => return string(value, mysql),
        Value::Json(value) => return string(&value.to_string(), mysql),
        Value::Bytes(value) => {
            let mut hex = String::from("x'");
            for byte in value {
                write!(hex, "{byte:02x}").expect("writing to a String is infallible");
            }
            hex.push('\'');
            hex
        }
    })
}

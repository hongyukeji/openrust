use std::{fmt::Write as _, sync::Arc};

use crate::database::{DatabaseError, DatabaseResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dialect {
    MySql,
    Sqlite,
}

#[derive(Clone, Copy)]
pub(in crate::database::query) enum DatePart {
    Date,
    Time,
    Day,
    Month,
    Year,
}

/// Compiles identifiers and pagination for the connection's SQL dialect.
#[derive(Clone)]
pub(crate) struct Grammar {
    dialect: Dialect,
    prefix: Arc<str>,
    upsert_alias: bool,
}

impl Grammar {
    pub(crate) fn mysql(prefix: &str, upsert_alias: bool) -> Self {
        Self {
            dialect: Dialect::MySql,
            upsert_alias,
            prefix: prefix.into(),
        }
    }

    pub(crate) fn sqlite(prefix: &str) -> Self {
        Self {
            dialect: Dialect::Sqlite,
            upsert_alias: false,
            prefix: prefix.into(),
        }
    }

    pub(crate) fn quote(&self, name: &str) -> DatabaseResult<String> {
        if name.is_empty() || name.contains('\0') {
            return Err(DatabaseError::InvalidQuery("identifier"));
        }
        let delimiter = match self.dialect {
            Dialect::MySql => '`',
            Dialect::Sqlite => '"',
        };
        Ok(format!(
            "{delimiter}{}{delimiter}",
            name.replace(delimiter, &delimiter.to_string().repeat(2))
        ))
    }

    fn segments(&self, name: &str, table: bool) -> DatabaseResult<String> {
        let segments: Vec<_> = name.split('.').collect();
        if segments.iter().any(|part| part.is_empty()) {
            return Err(DatabaseError::InvalidQuery("identifier"));
        }
        let prefixed = if table {
            segments.len().checked_sub(1)
        } else {
            (segments.len() > 1).then_some(0)
        };
        segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                if !table && index + 1 == segments.len() && *segment == "*" {
                    return Ok("*".to_owned());
                }
                if Some(index) == prefixed && !self.prefix.is_empty() {
                    return self.quote(&format!("{}{segment}", self.prefix));
                }
                self.quote(segment)
            })
            .collect::<DatabaseResult<Vec<_>>>()
            .map(|segments| segments.join("."))
    }

    fn wrap(&self, name: &str, table: bool, aliases: bool) -> DatabaseResult<String> {
        if aliases
            && let Some(index) = name
                .as_bytes()
                .windows(4)
                .position(|part| part.eq_ignore_ascii_case(b" as "))
        {
            let (name, alias) = name.split_at(index);
            let alias = &alias[4..];
            if alias.is_empty() {
                return Err(DatabaseError::InvalidQuery("identifier"));
            }
            let alias = if table {
                self.quote(&format!("{}{alias}", self.prefix))?
            } else {
                self.quote(alias)?
            };
            return Ok(format!("{} as {alias}", self.segments(name, table)?));
        }
        self.segments(name, table)
    }

    pub(crate) fn table(&self, name: &str) -> DatabaseResult<String> {
        self.wrap(name, true, true)
    }

    pub(crate) fn column(&self, name: &str) -> DatabaseResult<String> {
        self.wrap(name, false, false)
    }

    pub(crate) fn projection(&self, name: &str) -> DatabaseResult<String> {
        self.wrap(name, false, true)
    }

    pub(crate) fn null_safe_operator(&self) -> &'static str {
        match self.dialect {
            Dialect::MySql => "<=>",
            Dialect::Sqlite => "is",
        }
    }

    pub(in crate::database::query) fn date_part(
        &self,
        part: DatePart,
        column: &str,
        operator: &str,
    ) -> DatabaseResult<String> {
        let column = self.column(column)?;
        let (function, format) = match part {
            DatePart::Date => ("date", "%Y-%m-%d"),
            DatePart::Time => ("time", "%H:%M:%S"),
            DatePart::Day => ("day", "%d"),
            DatePart::Month => ("month", "%m"),
            DatePart::Year => ("year", "%Y"),
        };
        Ok(match self.dialect {
            Dialect::MySql => format!("{function}({column}) {operator} ?"),
            Dialect::Sqlite => format!("strftime('{format}', {column}) {operator} cast(? as text)"),
        })
    }

    pub(crate) fn like(&self, pattern: &str, case_sensitive: bool) -> (&'static str, String) {
        match (self.dialect, case_sensitive) {
            (_, false) => ("like", pattern.to_owned()),
            (Dialect::MySql, true) => ("like binary", pattern.to_owned()),
            (Dialect::Sqlite, true) => {
                let mut glob = String::with_capacity(pattern.len());
                for character in pattern.chars() {
                    match character {
                        '*' => glob.push_str("[*]"),
                        '?' => glob.push_str("[?]"),
                        '%' => glob.push('*'),
                        '_' => glob.push('?'),
                        other => glob.push(other),
                    }
                }
                ("glob", glob)
            }
        }
    }

    pub(crate) fn table_prefix(&self) -> &str {
        &self.prefix
    }

    pub(crate) fn is_mysql(&self) -> bool {
        self.dialect == Dialect::MySql
    }

    pub(crate) fn uses_upsert_alias(&self) -> bool {
        self.upsert_alias
    }

    pub(crate) fn lock(&self, exclusive: bool) -> &'static str {
        match (self.dialect, exclusive) {
            (Dialect::MySql, true) => " for update",
            (Dialect::MySql, false) => " lock in share mode",
            (Dialect::Sqlite, _) => "",
        }
    }

    pub(crate) fn wrap_union(&self, sql: &str) -> String {
        match self.dialect {
            Dialect::MySql => format!("({sql})"),
            Dialect::Sqlite => format!("select * from ({sql})"),
        }
    }

    pub(crate) fn pagination(sql: &mut String, limit: Option<u64>, offset: Option<u64>) {
        if let Some(limit) = limit {
            write!(sql, " limit {limit}").expect("writing to a String");
        }
        if let Some(offset) = offset {
            write!(sql, " offset {offset}").expect("writing to a String");
        }
    }
}

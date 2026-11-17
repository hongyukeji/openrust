mod json_predicates;

use std::{fmt::Write as _, sync::Arc};

use crate::database::{
    DatabaseError, DatabaseResult,
    query::{FullTextOptions, PostgresFullTextMode},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dialect {
    MySql,
    Sqlite,
    Postgres,
    SqlServer,
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
    mariadb: bool,
    truncate_cascade: bool,
}

impl Grammar {
    pub(crate) fn mysql(prefix: &str, upsert_alias: bool) -> Self {
        Self {
            dialect: Dialect::MySql,
            upsert_alias,
            prefix: prefix.into(),
            mariadb: false,
            truncate_cascade: false,
        }
    }

    pub(crate) fn mariadb(prefix: &str) -> Self {
        Self {
            mariadb: true,
            ..Self::mysql(prefix, false)
        }
    }

    pub(crate) fn sqlite(prefix: &str) -> Self {
        Self {
            dialect: Dialect::Sqlite,
            upsert_alias: false,
            prefix: prefix.into(),
            mariadb: false,
            truncate_cascade: false,
        }
    }

    pub(crate) fn postgres(prefix: &str) -> Self {
        Self {
            dialect: Dialect::Postgres,
            upsert_alias: false,
            prefix: prefix.into(),
            mariadb: false,
            truncate_cascade: true,
        }
    }

    pub(crate) fn with_truncate_cascade(mut self, cascade: bool) -> Self {
        self.truncate_cascade = cascade;
        self
    }

    pub(in crate::database::query) fn postgres_truncate(&self, target: &str) -> String {
        let cascade = if self.truncate_cascade {
            " cascade"
        } else {
            ""
        };
        format!("truncate table {target} restart identity{cascade}")
    }

    pub(crate) fn is_mariadb(&self) -> bool {
        self.mariadb
    }

    pub(crate) fn sql_server(prefix: &str) -> Self {
        Self {
            dialect: Dialect::SqlServer,
            ..Self::postgres(prefix)
        }
    }

    pub(crate) fn is_sql_server(&self) -> bool {
        self.dialect == Dialect::SqlServer
    }

    pub(crate) fn is_postgres(&self) -> bool {
        self.dialect == Dialect::Postgres
    }

    pub(crate) fn quote(&self, name: &str) -> DatabaseResult<String> {
        if name.is_empty() || name.contains('\0') {
            return Err(DatabaseError::InvalidQuery("identifier"));
        }
        let delimiter = match self.dialect {
            Dialect::MySql => '`',
            Dialect::Sqlite | Dialect::Postgres => '"',
            Dialect::SqlServer => return Ok(format!("[{}]", name.replace(']', "]]"))),
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

    pub(in crate::database::query) fn table_in_database(
        &self,
        name: &str,
        database: &str,
    ) -> DatabaseResult<String> {
        let table = self.table(name)?;
        let relation = name
            .as_bytes()
            .windows(4)
            .position(|part| part.eq_ignore_ascii_case(b" as "))
            .map_or(name, |alias| &name[..alias]);
        if relation.contains('.') {
            return Ok(table);
        }
        Ok(format!("{}.{table}", self.quote(database)?))
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
            Dialect::Postgres => "is not distinct from",
            Dialect::SqlServer => "=",
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
            Dialect::Postgres | Dialect::SqlServer
                if matches!(part, DatePart::Date | DatePart::Time) =>
            {
                format!("cast({column} as {function}) {operator} ?")
            }
            Dialect::Postgres => format!("extract({function} from {column}) {operator} ?"),
            Dialect::SqlServer => format!("datepart({function}, {column}) {operator} ?"),
            Dialect::Sqlite => format!("strftime('{format}', {column}) {operator} cast(? as text)"),
        })
    }

    pub(in crate::database::query) fn vector_distance(
        &self,
        column: &str,
    ) -> DatabaseResult<String> {
        if self.is_postgres() {
            // SQLx binds the serialized vector as text. Let PostgreSQL parse the vector.
            Ok(format!("({column} <=> cast(cast(? as text) as vector))"))
        } else if self.is_mariadb() {
            Ok(format!("vec_distance_cosine({column}, vec_fromtext(?))"))
        } else {
            Err(DatabaseError::UnsupportedOperation(
                "vector distance queries",
            ))
        }
    }

    pub(in crate::database::query) fn full_text(
        &self,
        columns: &[String],
        options: FullTextOptions,
    ) -> DatabaseResult<String> {
        if self.is_postgres() {
            let (vectors, tsquery) = self.postgres_full_text(columns, options, false)?;
            return Ok(format!("({vectors}) @@ {tsquery}"));
        }
        if !self.is_mysql() {
            return Err(DatabaseError::InvalidQuery(
                "full-text search is unavailable for this database driver",
            ));
        }
        if columns.is_empty()
            || columns
                .iter()
                .any(|column| column.rsplit('.').next() == Some("*"))
        {
            return Err(DatabaseError::InvalidQuery(
                "full-text search requires indexed columns",
            ));
        }
        let columns = columns
            .iter()
            .map(|column| self.column(column))
            .collect::<DatabaseResult<Vec<_>>>()?;
        let options = match options {
            FullTextOptions::NaturalLanguage => "in natural language mode",
            FullTextOptions::Boolean => "in boolean mode",
            FullTextOptions::QueryExpansion => "in natural language mode with query expansion",
            FullTextOptions::Postgres { .. } => {
                return Err(DatabaseError::UnsupportedOperation(
                    "PostgreSQL full-text options on MySQL",
                ));
            }
        };
        Ok(format!(
            "match ({}) against (? {options})",
            columns.join(", ")
        ))
    }

    pub(crate) fn postgres_text_vector(
        &self,
        column: &str,
        language: &str,
    ) -> DatabaseResult<String> {
        let quoted_language = language.replace('\'', "''");
        Ok(format!(
            "to_tsvector('{quoted_language}', {})",
            self.column(column)?
        ))
    }

    fn postgres_full_text(
        &self,
        columns: &[String],
        options: FullTextOptions,
        rank: bool,
    ) -> DatabaseResult<(String, String)> {
        if columns.is_empty() || columns.iter().any(|c| c.rsplit('.').next() == Some("*")) {
            return Err(DatabaseError::InvalidQuery(
                "full-text search requires indexed columns",
            ));
        }
        let (language, mode, vector) = match options {
            FullTextOptions::NaturalLanguage => ("english", PostgresFullTextMode::Plain, false),
            FullTextOptions::Postgres {
                language,
                mode,
                vector,
            } => (language, mode, vector),
            _ => {
                return Err(DatabaseError::UnsupportedOperation(
                    "MySQL full-text options on PostgreSQL",
                ));
            }
        };
        const LANGUAGES: &[&str] = &[
            "simple",
            "arabic",
            "armenian",
            "basque",
            "catalan",
            "danish",
            "dutch",
            "english",
            "finnish",
            "french",
            "german",
            "greek",
            "hindi",
            "hungarian",
            "indonesian",
            "irish",
            "italian",
            "lithuanian",
            "nepali",
            "norwegian",
            "portuguese",
            "romanian",
            "russian",
            "serbian",
            "spanish",
            "swedish",
            "tamil",
            "turkish",
            "yiddish",
        ];
        let language = if LANGUAGES.contains(&language) {
            language
        } else {
            "english"
        };
        let vectors = columns
            .iter()
            .map(|column| {
                let expr = if vector {
                    self.column(column)?
                } else {
                    self.postgres_text_vector(column, language)?
                };
                Ok(if rank {
                    format!("coalesce({expr}, ''::tsvector)")
                } else {
                    expr
                })
            })
            .collect::<DatabaseResult<Vec<_>>>()?
            .join(" || ");
        let mode = match mode {
            PostgresFullTextMode::Plain => "plainto_tsquery",
            PostgresFullTextMode::Phrase => "phraseto_tsquery",
            PostgresFullTextMode::WebSearch => "websearch_to_tsquery",
            PostgresFullTextMode::Raw => "to_tsquery",
        };
        Ok((vectors, format!("{mode}('{language}', ?)")))
    }

    pub(in crate::database::query) fn postgres_full_text_rank(
        &self,
        columns: &[String],
        options: FullTextOptions,
    ) -> DatabaseResult<String> {
        let (vectors, query) = self.postgres_full_text(columns, options, true)?;
        Ok(format!("ts_rank({vectors}, {query}) desc"))
    }

    pub(crate) fn like(&self, pattern: &str, case_sensitive: bool) -> (&'static str, String) {
        match (self.dialect, case_sensitive) {
            (Dialect::Postgres, false) => ("ilike", pattern.to_owned()),
            (_, false) | (Dialect::Postgres | Dialect::SqlServer, true) => {
                ("like", pattern.to_owned())
            }
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

    pub(crate) fn table_prefix(&self) -> &Arc<str> {
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
            (Dialect::MySql | Dialect::Postgres, true) => " for update",
            (Dialect::Postgres, false) => " for share",
            (Dialect::MySql, false) => " lock in share mode",
            (Dialect::Sqlite | Dialect::SqlServer, _) => "",
        }
    }

    pub(crate) fn wrap_union(&self, sql: &str) -> String {
        match self.dialect {
            Dialect::MySql | Dialect::Postgres => format!("({sql})"),
            Dialect::Sqlite => format!("select * from ({sql})"),
            Dialect::SqlServer => format!("select * from ({sql}) as [openrust_union]"),
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

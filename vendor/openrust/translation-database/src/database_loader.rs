#![allow(non_snake_case, reason = "保留既定翻译接口命名")]

use std::{
    collections::{BTreeSet, btree_map::Entry},
    io,
    sync::Arc,
};

use openrust::{
    contracts::translation::Loader,
    database::{Connection, Value},
    translation::{
        Line, Lines, TranslationError, TranslationFuture, TranslationResult, canonical_locale,
    },
};

/// Explicit, read-only translation source using an application's SQL connection.
///
/// The `language_lines` table contains `group`, `key`, and a `text` JSON object
/// mapping canonical locales to strings. Package namespaces use the file source.
///
/// ```no_run
/// use openrust::{database::Connection, translation::{Translator, TranslationResource, TranslationResult}};
/// use openrust_translation_database::DatabaseLoader;
///
/// async fn publish(connection: Connection, translator: &Translator) -> TranslationResult<()> {
///     let loader = DatabaseLoader::new(connection).withTable("language_lines")?;
///     translator.loadFrom(&loader, &[TranslationResource {
///         locale: "en", group: "messages", namespace: None,
///     }]).await
/// }
/// ```
#[derive(Clone)]
pub struct DatabaseLoader {
    connection: Connection,
    table: String,
    max_rows: u64,
    fallback: Option<Arc<dyn Loader>>,
}

impl DatabaseLoader {
    /// Uses the `language_lines` table, including the connection's configured prefix.
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            table: "language_lines".into(),
            max_rows: 10_000,
            fallback: None,
        }
    }

    /// Selects an unqualified table name; this does not create or migrate the table.
    pub fn with_table(mut self, table: impl Into<String>) -> TranslationResult<Self> {
        let table = table.into();
        let mut bytes = table.bytes();
        if !bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
            || !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(TranslationError::InvalidKey);
        }
        self.table = table;
        Ok(self)
    }

    /// Bounds each SQL group load. Exceeding the limit fails without truncation.
    pub fn with_max_rows(mut self, rows: u64) -> TranslationResult<Self> {
        if rows == 0 || rows == u64::MAX {
            return Err(invalid("Invalid translation row limit"));
        }
        self.max_rows = rows;
        Ok(self)
    }

    /// Merges database entries over another exact-locale source, such as FileLoader.
    ///
    /// A database error still fails the load; it never silently returns the fallback.
    pub fn with_fallback(mut self, fallback: Arc<dyn Loader>) -> Self {
        self.fallback = Some(fallback);
        self
    }
}

impl Loader for DatabaseLoader {
    fn load<'a>(
        &'a self,
        locale: &'a str,
        group: &'a str,
        namespace: Option<&'a str>,
    ) -> TranslationFuture<'a, TranslationResult<Lines>> {
        Box::pin(async move {
            let locale = canonical_locale(locale)?;
            let namespace = namespace.filter(|namespace| *namespace != "*");
            let namespace = if group == "*" && namespace.is_none() {
                Some("*")
            } else {
                if !group.split('/').all(valid_segment)
                    || namespace.is_some_and(|namespace| !valid_segment(namespace))
                {
                    return Err(TranslationError::InvalidKey);
                }
                namespace
            };
            // Spatie's loader leaves package namespaces to the file loader.
            if namespace.is_some_and(|namespace| namespace != "*") {
                return match &self.fallback {
                    Some(fallback) => fallback.load(&locale, group, namespace).await,
                    None => Ok(Lines::new()),
                };
            }
            let rows = self
                .connection
                .table(&self.table)
                .select(["group", "key", "text"])
                .r#where("group", "=", group)
                .limit(self.max_rows + 1)
                .get()
                .await
                .map_err(source)?;
            if u64::try_from(rows.len()).unwrap_or(u64::MAX) > self.max_rows {
                return Err(invalid("Translation group exceeds its row limit"));
            }
            let mut incoming = Lines::new();
            let mut keys = BTreeSet::<String>::new();
            for row in rows {
                // Do not let a database's case-insensitive collation mix resources.
                if row.get("group") != Some(&Value::from(group)) {
                    continue;
                }
                let Some(Value::Text(key)) = row.get("key") else {
                    return Err(invalid("Translation key must be text"));
                };
                if key.is_empty() {
                    return Err(invalid("Translation key must not be empty"));
                }
                let text: serde_json::Value = match row.get("text") {
                    Some(Value::Text(text)) => serde_json::from_str(text).map_err(source)?,
                    Some(Value::Json(text)) => text.clone(),
                    _ => return Err(invalid("Translation text must contain JSON")),
                };
                let Some(text) = text.as_object() else {
                    return Err(invalid("Translation text must be a locale map"));
                };
                // Locale fallback belongs to the existing Translator, not this source.
                let value = match text.get(&locale) {
                    None | Some(serde_json::Value::Null) => continue,
                    Some(serde_json::Value::String(value)) => Line::Text(value.clone()),
                    _ => return Err(invalid("Translation must be a string or null")),
                };
                let segments = if group == "*" {
                    vec![key.as_str()]
                } else {
                    key.split('.').collect()
                };
                if segments.len() > 128 {
                    return Err(invalid("Translation key exceeds its nesting limit"));
                }
                if segments.iter().any(|segment| segment.is_empty()) {
                    return Err(invalid("Translation key contains an empty segment"));
                }
                if group != "*" {
                    let mut parent = String::new();
                    for segment in &segments[..segments.len() - 1] {
                        if !parent.is_empty() {
                            parent.push('.');
                        }
                        parent.push_str(segment);
                        if keys.contains(&parent) {
                            return Err(invalid("Overlapping translation key"));
                        }
                    }
                    let descendant = format!("{key}.");
                    if keys
                        .range(descendant.clone()..)
                        .next()
                        .is_some_and(|key| key.starts_with(&descendant))
                    {
                        return Err(invalid("Overlapping translation key"));
                    }
                }
                if !keys.insert(key.clone()) {
                    return Err(invalid("Duplicate translation key"));
                }
                insert(&mut incoming, &segments, value)?;
            }
            let mut lines = match &self.fallback {
                Some(fallback) => fallback.load(&locale, group, namespace).await?,
                None => Lines::new(),
            };
            for (key, value) in incoming {
                if group == "*" {
                    lines.insert(key, value);
                } else if let Some(previous) = lines.get_mut(&key) {
                    merge(previous, value);
                } else {
                    lines.insert(key, value);
                }
            }
            Ok(lines)
        })
    }
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn insert(lines: &mut Lines, segments: &[&str], value: Line) -> TranslationResult<()> {
    if segments.len() == 1 {
        return match lines.entry(segments[0].to_owned()) {
            Entry::Vacant(entry) => {
                entry.insert(value);
                Ok(())
            }
            Entry::Occupied(_) => Err(invalid("Duplicate or overlapping translation key")),
        };
    }
    let child = lines
        .entry(segments[0].to_owned())
        .or_insert_with(|| Line::Group(Lines::new()));
    let Line::Group(child) = child else {
        return Err(invalid("Overlapping translation key"));
    };
    insert(child, &segments[1..], value)
}

// Follow the FileLoader's recursive, index-preserving group merge semantics.
fn merge(current: &mut Line, incoming: Line) {
    match (current, incoming) {
        (Line::Group(current), Line::Group(incoming)) => merge_groups(current, incoming),
        (Line::List(current), Line::List(incoming)) => {
            for (index, value) in incoming.into_iter().enumerate() {
                if let Some(previous) = current.get_mut(index) {
                    merge(previous, value);
                } else {
                    current.push(value);
                }
            }
        }
        (Line::Group(current), Line::List(incoming)) => merge_groups(current, indexed(incoming)),
        (current @ Line::List(_), Line::Group(incoming)) => {
            let Line::List(items) = std::mem::replace(current, Line::Group(Lines::new())) else {
                unreachable!()
            };
            let mut entries = indexed(items);
            merge_groups(&mut entries, incoming);
            *current = Line::Group(entries);
        }
        (current, incoming) => *current = incoming,
    }
}

fn indexed(lines: Vec<Line>) -> Lines {
    lines
        .into_iter()
        .enumerate()
        .map(|(i, value)| (i.to_string(), value))
        .collect()
}

fn merge_groups(current: &mut Lines, incoming: Lines) {
    for (key, value) in incoming {
        if let Some(previous) = current.get_mut(&key) {
            merge(previous, value);
        } else {
            current.insert(key, value);
        }
    }
}

fn invalid(message: &'static str) -> TranslationError {
    source(io::Error::new(io::ErrorKind::InvalidData, message))
}

fn source(error: impl std::error::Error + Send + Sync + 'static) -> TranslationError {
    TranslationError::Source(Box::new(error))
}

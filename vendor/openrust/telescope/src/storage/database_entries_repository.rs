use super::EntryQueryOptions;
use crate::{EntryResult, EntryType, EntryUpdate, Error, IncomingEntry, Result};
use openrust::database::{Connection, DatabaseError, Row, Value};
use std::{collections::BTreeMap, sync::Mutex};
use uuid::Uuid;

pub struct DatabaseEntriesRepository {
    connection: Connection,
    chunk_size: usize,
    monitored_tags: Mutex<Option<Vec<String>>>,
}
impl DatabaseEntriesRepository {
    pub fn new(connection: Connection, chunk_size: Option<usize>) -> Self {
        Self {
            connection,
            chunk_size: chunk_size.filter(|size| *size > 0).unwrap_or(1000),
            monitored_tags: Mutex::new(None),
        }
    }
    pub async fn find(&self, id: &str) -> Result<EntryResult> {
        let query = self.connection.table("telescope_entries");
        let query =
            if id.len() < 36 && !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                query
                    .r#where("uuid", "like", format!("{id}%"))
                    .order_by_desc("sequence")
            } else {
                query.where_exact_text("uuid", id)
            };
        let row = query.first().await?.ok_or(Error::NotFound)?;
        let mut entry = decode(&row)?.ok_or(Error::InvalidEntry)?;
        entry.sequence = None;
        entry.tags = self
            .connection
            .table("telescope_entries_tags")
            .where_exact_text("entry_uuid", &entry.id.to_string())
            .get()
            .await?
            .iter()
            .map(|row| text(row, "tag"))
            .collect::<Result<_>>()?;
        Ok(entry)
    }
    pub async fn get(
        &self,
        kind: Option<EntryType>,
        options: EntryQueryOptions,
    ) -> Result<Vec<EntryResult>> {
        let mut query = self.connection.table("telescope_entries");
        if let Some(kind) = kind {
            query = query.where_exact_text("type", kind.as_str());
        }
        if let Some(batch) = options.batch_id {
            query = query.where_exact_text("batch_id", &batch.to_string());
        }
        let tag = options
            .tag
            .as_deref()
            .filter(|value| super::has_string_filter(value));
        if let Some(tag) = tag {
            let mut tags = self
                .connection
                .table("telescope_entries_tags")
                .select(["entry_uuid"])
                .where_in(
                    "tag",
                    tag.split(',')
                        .map(|tag| {
                            Value::from(tag.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b']))
                        })
                        .collect(),
                );
            query = query.where_in_sub("uuid", &mut tags)?;
        }
        let family = options
            .family_hash
            .as_deref()
            .filter(|value| super::has_string_filter(value));
        if let Some(hash) = family {
            query = query.where_exact_text("family_hash", hash);
        }
        if let Some(before) = options.before_sequence {
            query = query.r#where("sequence", "<", before);
        }
        if let Some(ids) = options.uuids {
            query = query.where_in(
                "uuid",
                ids.iter().map(|id| Value::from(id.to_string())).collect(),
            );
        }
        if options.batch_id.is_none() && tag.is_none() && family.is_none() {
            query = query.r#where("should_display_on_index", "=", true);
        }
        query = query.order_by_desc("sequence");
        if options.limit != u64::MAX {
            query = query.limit(options.limit);
        }
        let rows = query.get().await?;
        rows.iter()
            .map(decode)
            .filter_map(|result| result.transpose())
            .collect()
    }
    pub async fn store(&self, entries: &[IncomingEntry]) -> Result<()> {
        // Validate the entire batch before the first mutation.
        for entry in entries {
            row(entry)?;
        }
        let exceptions: Vec<_> = entries
            .iter()
            .filter(|entry| entry.is_exception())
            .collect();
        let ordinary: Vec<_> = entries
            .iter()
            .filter(|entry| !entry.is_exception())
            .collect();
        for chunk in exceptions.chunks(self.chunk_size) {
            let mut rows = Vec::new();
            for entry in chunk {
                let hash = entry.family_hash.as_deref().ok_or(Error::InvalidEntry)?;
                let occurrences = self
                    .connection
                    .table("telescope_entries")
                    .where_exact_text("type", "exception")
                    .where_exact_text("family_hash", hash)
                    .count("*")
                    .await?;
                self.connection
                    .table("telescope_entries")
                    .where_exact_text("type", "exception")
                    .where_exact_text("family_hash", hash)
                    .r#where("should_display_on_index", "=", true)
                    .update(BTreeMap::from([(
                        "should_display_on_index".into(),
                        Value::Bool(false),
                    )]))
                    .await?;
                let mut record = row(entry)?;
                let mut content = entry.content.clone();
                content.insert("occurrences".into(), (occurrences + 1).into());
                record.insert(
                    "content".into(),
                    serde_json::Value::Object(content).to_string().into(),
                );
                rows.push(record);
            }
            self.connection
                .table("telescope_entries")
                .insert(&rows)
                .await?;
        }
        self.store_tags(
            exceptions
                .iter()
                .map(|entry| (entry.uuid, entry.tags.clone()))
                .collect(),
        )
        .await?;
        for chunk in ordinary.chunks(self.chunk_size) {
            let rows = chunk
                .iter()
                .map(|entry| row(entry))
                .collect::<Result<Vec<_>>>()?;
            self.connection
                .table("telescope_entries")
                .insert(&rows)
                .await?;
        }
        self.store_tags(
            ordinary
                .iter()
                .map(|entry| (entry.uuid, entry.tags.clone()))
                .collect(),
        )
        .await
    }
    async fn store_tags(&self, entries: Vec<(Uuid, Vec<String>)>) -> Result<()> {
        let mut rows = Vec::new();
        for (uuid, tags) in entries {
            for tag in tags {
                rows.push(BTreeMap::from([
                    ("entry_uuid".into(), Value::from(uuid.to_string())),
                    ("tag".into(), Value::from(tag.clone())),
                ]));
                if rows.len() >= self.chunk_size {
                    self.insert_tags(&rows).await?;
                    rows.clear();
                }
            }
        }
        if !rows.is_empty() {
            self.insert_tags(&rows).await?;
        }
        Ok(())
    }
    async fn insert_tags(&self, rows: &[BTreeMap<String, Value>]) -> Result<()> {
        match self
            .connection
            .table("telescope_entries_tags")
            .insert(rows)
            .await
        {
            Ok(_) => Ok(()),
            Err(DatabaseError::Driver { source, .. })
                if source
                    .as_database_error()
                    .is_some_and(|error| error.is_unique_violation()) =>
            {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }
    pub async fn update(&self, updates: &[EntryUpdate]) -> Result<Vec<EntryUpdate>> {
        let mut failed = Vec::new();
        for update in updates {
            let query = self
                .connection
                .table("telescope_entries")
                .where_exact_text("uuid", &update.uuid.to_string())
                .where_exact_text("type", update.r#type.as_str());
            let Some(record) = query.first().await? else {
                failed.push(update.clone());
                continue;
            };
            let mut content = json(&record)?.as_object().cloned().unwrap_or_default();
            content.extend(update.changes.clone());
            self.connection
                .table("telescope_entries")
                .where_exact_text("uuid", &update.uuid.to_string())
                .where_exact_text("type", update.r#type.as_str())
                .update(BTreeMap::from([(
                    "content".into(),
                    serde_json::Value::Object(content).to_string().into(),
                )]))
                .await?;
            self.store_tags(vec![(update.uuid, update.added_tags.clone())])
                .await?;
            if !update.removed_tags.is_empty() {
                self.connection
                    .table("telescope_entries_tags")
                    .where_exact_text("entry_uuid", &update.uuid.to_string())
                    .where_in(
                        "tag",
                        update
                            .removed_tags
                            .iter()
                            .cloned()
                            .map(Value::from)
                            .collect(),
                    )
                    .delete()
                    .await?;
            }
        }
        Ok(failed)
    }
    pub async fn monitoring(&self) -> Result<Vec<String>> {
        self.connection
            .table("telescope_monitoring")
            .get()
            .await?
            .iter()
            .map(|row| text(row, "tag"))
            .collect()
    }
    pub async fn load_monitored_tags(&self) {
        let tags = self.monitoring().await.unwrap_or_default();
        *self
            .monitored_tags
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(tags);
    }
    pub(crate) fn has_monitored_tag(&self, tags: &[String]) -> bool {
        self.monitored_tags
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|monitored| tags.iter().any(|tag| monitored.contains(tag)))
    }
    pub async fn is_monitoring(&self, tags: &[String]) -> bool {
        let cached = self
            .monitored_tags
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let monitored = match cached {
            Some(tags) => tags,
            None => {
                self.load_monitored_tags().await;
                self.monitored_tags
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone()
                    .unwrap_or_default()
            }
        };
        tags.iter().any(|tag| monitored.contains(tag))
    }
    pub async fn monitor(&self, tags: &[String]) -> Result<()> {
        let existing = self.monitoring().await?;
        let mut rows = Vec::new();
        for tag in tags {
            let row = BTreeMap::from([("tag".into(), Value::from(tag.clone()))]);
            if !existing.contains(tag) && !rows.contains(&row) {
                rows.push(row);
            }
        }
        self.connection
            .table("telescope_monitoring")
            .insert(&rows)
            .await?;
        Ok(())
    }
    pub async fn stop_monitoring(&self, tags: &[String]) -> Result<()> {
        self.connection
            .table("telescope_monitoring")
            .where_in("tag", tags.iter().cloned().map(Value::from).collect())
            .delete()
            .await?;
        Ok(())
    }
    pub async fn prune(&self, before: jiff::Timestamp, keep_exceptions: bool) -> Result<u64> {
        let mut total = 0;
        loop {
            let mut query = self.connection.table("telescope_entries").r#where(
                "created_at",
                "<",
                timestamp(before),
            );
            if keep_exceptions {
                query = query.r#where("type", "!=", "exception");
            }
            let ids = query
                .select(["sequence"])
                .order_by("sequence", "asc")
                .limit(self.chunk_size as u64)
                .get()
                .await?
                .iter()
                .filter_map(|row| row.get("sequence").cloned())
                .collect::<Vec<_>>();
            if ids.is_empty() {
                break;
            }
            total += self
                .connection
                .table("telescope_entries")
                .where_in("sequence", ids)
                .delete()
                .await?;
        }
        Ok(total)
    }
    pub async fn clear(&self) -> Result<()> {
        loop {
            let ids = self
                .connection
                .table("telescope_entries")
                .select(["sequence"])
                .order_by("sequence", "asc")
                .limit(self.chunk_size as u64)
                .get()
                .await?
                .iter()
                .filter_map(|row| row.get("sequence").cloned())
                .collect::<Vec<_>>();
            if ids.is_empty() {
                break;
            }
            self.connection
                .table("telescope_entries")
                .where_in("sequence", ids)
                .delete()
                .await?;
        }
        loop {
            let tags = self
                .connection
                .table("telescope_monitoring")
                .select(["tag"])
                .order_by("tag", "asc")
                .limit(self.chunk_size as u64)
                .get()
                .await?
                .iter()
                .filter_map(|row| row.get("tag").cloned())
                .collect::<Vec<_>>();
            if tags.is_empty() {
                break;
            }
            self.connection
                .table("telescope_monitoring")
                .where_in("tag", tags)
                .delete()
                .await?;
        }
        Ok(())
    }
    pub fn terminate(&self) {
        *self
            .monitored_tags
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
    }
}
fn timestamp(timestamp: jiff::Timestamp) -> String {
    timestamp
        .to_zoned(jiff::tz::TimeZone::UTC)
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string()
}
fn row(entry: &IncomingEntry) -> Result<BTreeMap<String, Value>> {
    let kind = entry.r#type.ok_or(Error::InvalidEntry)?;
    let batch = entry.batch_id.ok_or(Error::InvalidEntry)?;
    if entry.is_exception() && entry.family_hash.is_none() {
        return Err(Error::InvalidEntry);
    }
    Ok(BTreeMap::from([
        ("uuid".into(), entry.uuid.to_string().into()),
        ("batch_id".into(), batch.to_string().into()),
        (
            "family_hash".into(),
            entry.family_hash.clone().map_or(Value::Null, Value::from),
        ),
        ("type".into(), kind.as_str().into()),
        (
            "content".into(),
            serde_json::Value::Object(entry.content.clone())
                .to_string()
                .into(),
        ),
        ("created_at".into(), timestamp(entry.recorded_at).into()),
    ]))
}
fn text(row: &Row, key: &str) -> Result<String> {
    match row.get(key) {
        Some(Value::Text(text)) => Ok(text.clone()),
        _ => Err(Error::InvalidEntry),
    }
}
fn json(row: &Row) -> Result<serde_json::Value> {
    match row.get("content") {
        Some(Value::Text(text)) => {
            Ok(serde_json::from_str(text).unwrap_or(serde_json::Value::Null))
        }
        Some(Value::Json(value)) => Ok(value.clone()),
        _ => Err(Error::InvalidEntry),
    }
}
fn decode(row: &Row) -> Result<Option<EntryResult>> {
    let content = json(row)?;
    if !content.is_object() && !content.is_array() {
        return Ok(None);
    }
    let sequence = match row.get("sequence") {
        Some(Value::Integer(sequence)) => *sequence,
        Some(Value::Unsigned(sequence)) => {
            (*sequence).try_into().map_err(|_| Error::InvalidEntry)?
        }
        _ => return Err(Error::InvalidEntry),
    };
    Ok(Some(EntryResult {
        id: text(row, "uuid")?
            .parse()
            .map_err(|_| Error::InvalidEntry)?,
        sequence: Some(sequence),
        batch_id: text(row, "batch_id")?
            .parse()
            .map_err(|_| Error::InvalidEntry)?,
        r#type: text(row, "type")?.parse()?,
        family_hash: match row.get("family_hash") {
            Some(Value::Null) => None,
            _ => Some(text(row, "family_hash")?),
        },
        content,
        created_at: text(row, "created_at")?,
        tags: Vec::new(),
    }))
}

impl crate::contracts::ClearableRepository for DatabaseEntriesRepository {
    fn clear(&self) -> crate::contracts::TelescopeFuture<'_, Result<()>> {
        Box::pin(DatabaseEntriesRepository::clear(self))
    }
}
impl crate::contracts::PrunableRepository for DatabaseEntriesRepository {
    fn prune(
        &self,
        before: jiff::Timestamp,
        keep_exceptions: bool,
    ) -> crate::contracts::TelescopeFuture<'_, Result<u64>> {
        Box::pin(DatabaseEntriesRepository::prune(
            self,
            before,
            keep_exceptions,
        ))
    }
}
impl crate::contracts::EntriesRepository for DatabaseEntriesRepository {
    fn find<'a>(
        &'a self,
        id: &'a str,
    ) -> crate::contracts::TelescopeFuture<'a, Result<EntryResult>> {
        Box::pin(DatabaseEntriesRepository::find(self, id))
    }
    fn get(
        &self,
        kind: Option<EntryType>,
        options: EntryQueryOptions,
    ) -> crate::contracts::TelescopeFuture<'_, Result<Vec<EntryResult>>> {
        Box::pin(DatabaseEntriesRepository::get(self, kind, options))
    }
    fn store<'a>(
        &'a self,
        entries: &'a [IncomingEntry],
    ) -> crate::contracts::TelescopeFuture<'a, Result<()>> {
        Box::pin(DatabaseEntriesRepository::store(self, entries))
    }
    fn update<'a>(
        &'a self,
        updates: &'a [EntryUpdate],
    ) -> crate::contracts::TelescopeFuture<'a, Result<Vec<EntryUpdate>>> {
        Box::pin(DatabaseEntriesRepository::update(self, updates))
    }
    fn monitoring(&self) -> crate::contracts::TelescopeFuture<'_, Result<Vec<String>>> {
        Box::pin(DatabaseEntriesRepository::monitoring(self))
    }
    fn load_monitored_tags(&self) -> crate::contracts::TelescopeFuture<'_, ()> {
        Box::pin(DatabaseEntriesRepository::load_monitored_tags(self))
    }
    fn has_monitored_tag(&self, tags: &[String]) -> bool {
        DatabaseEntriesRepository::has_monitored_tag(self, tags)
    }
    fn is_monitoring<'a>(
        &'a self,
        tags: &'a [String],
    ) -> crate::contracts::TelescopeFuture<'a, bool> {
        Box::pin(DatabaseEntriesRepository::is_monitoring(self, tags))
    }
    fn monitor<'a>(
        &'a self,
        tags: &'a [String],
    ) -> crate::contracts::TelescopeFuture<'a, Result<()>> {
        Box::pin(DatabaseEntriesRepository::monitor(self, tags))
    }
    fn stop_monitoring<'a>(
        &'a self,
        tags: &'a [String],
    ) -> crate::contracts::TelescopeFuture<'a, Result<()>> {
        Box::pin(DatabaseEntriesRepository::stop_monitoring(self, tags))
    }
    fn terminate(&self) {
        DatabaseEntriesRepository::terminate(self);
    }
}

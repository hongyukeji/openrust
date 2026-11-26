use uuid::Uuid;

pub(crate) fn has_string_filter(value: &str) -> bool {
    !matches!(value, "" | "0")
}

#[derive(Clone, Debug)]
pub struct EntryQueryOptions {
    pub batch_id: Option<Uuid>,
    pub tag: Option<String>,
    pub family_hash: Option<String>,
    pub before_sequence: Option<i64>,
    pub uuids: Option<Vec<Uuid>>,
    pub limit: u64,
}
impl Default for EntryQueryOptions {
    fn default() -> Self {
        Self {
            batch_id: None,
            tag: None,
            family_hash: None,
            before_sequence: None,
            uuids: None,
            limit: 50,
        }
    }
}
impl EntryQueryOptions {
    pub fn for_batch_id(id: Option<Uuid>) -> Self {
        Self::default().batch_id(id)
    }
    pub fn batch_id(mut self, id: Option<Uuid>) -> Self {
        self.batch_id = id;
        self
    }
    pub fn tag(mut self, tag: Option<String>) -> Self {
        self.tag = tag;
        self
    }
    pub fn family_hash(mut self, hash: Option<String>) -> Self {
        self.family_hash = hash;
        self
    }
    /// 整数零和 `None` 均表示不筛选。
    pub fn before_sequence(mut self, sequence: Option<i64>) -> Self {
        self.before_sequence = sequence.filter(|sequence| *sequence != 0);
        self
    }
    pub(crate) fn with_filter_inputs(
        mut self,
        batch: Option<String>,
        before: Option<String>,
    ) -> crate::Result<Self> {
        if let Some(batch) = batch {
            self.batch_id = Some(batch.parse().map_err(|_| crate::Error::InvalidEntry)?);
        }
        if let Some(before) = before {
            self.before_sequence = Some(before.parse().map_err(|_| crate::Error::InvalidEntry)?);
        }
        Ok(self)
    }
    pub fn uuids(mut self, uuids: Option<Vec<Uuid>>) -> Self {
        self.uuids = uuids;
        self
    }
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }
}

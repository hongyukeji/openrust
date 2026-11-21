use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct EntryQueryOptions {
    pub batchId: Option<Uuid>,
    pub tag: Option<String>,
    pub familyHash: Option<String>,
    pub beforeSequence: Option<i64>,
    pub uuids: Option<Vec<Uuid>>,
    pub limit: u64,
}
impl Default for EntryQueryOptions {
    fn default() -> Self {
        Self {
            batchId: None,
            tag: None,
            familyHash: None,
            beforeSequence: None,
            uuids: None,
            limit: 50,
        }
    }
}
impl EntryQueryOptions {
    pub fn forBatchId(id: Option<Uuid>) -> Self {
        Self::default().batchId(id)
    }
    pub fn batchId(mut self, id: Option<Uuid>) -> Self {
        self.batchId = id;
        self
    }
    pub fn tag(mut self, tag: Option<String>) -> Self {
        self.tag = tag;
        self
    }
    pub fn familyHash(mut self, hash: Option<String>) -> Self {
        self.familyHash = hash;
        self
    }
    pub fn beforeSequence(mut self, sequence: Option<i64>) -> Self {
        self.beforeSequence = sequence;
        self
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

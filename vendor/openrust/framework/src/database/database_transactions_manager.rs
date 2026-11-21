use std::{cmp::Reverse, sync::Mutex};

use super::{DatabaseError, DatabaseFuture, DatabaseResult};

type TransactionCallback = Box<dyn FnOnce() -> DatabaseFuture<'static, DatabaseResult<()>> + Send>;

#[derive(Default)]
struct TransactionRecord {
    level: u32,
    callbacks: Vec<TransactionCallback>,
    rollback_callbacks: Vec<TransactionCallback>,
    committed_children: Vec<TransactionRecord>,
}

impl TransactionRecord {
    fn stage(mut self, records: &mut Vec<Self>) {
        for child in std::mem::take(&mut self.committed_children) {
            child.stage(records);
        }
        records.push(self);
    }

    fn collect_rollback_callbacks(mut self, callbacks: &mut Vec<TransactionCallback>) {
        Self::rollback_children(self.committed_children, callbacks);
        callbacks.append(&mut self.rollback_callbacks);
    }

    fn rollback_children(mut children: Vec<Self>, callbacks: &mut Vec<TransactionCallback>) {
        for child in &mut children {
            Self::rollback_children(std::mem::take(&mut child.committed_children), callbacks);
        }
        for mut child in children {
            callbacks.append(&mut child.rollback_callbacks);
        }
    }
}

/// Associates transaction callbacks with one transaction's savepoints.
#[derive(Default)]
pub struct DatabaseTransactionsManager {
    // The transaction owns these records exclusively; the mutex keeps its
    // shared query borrows Sync even when a callback is only Send.
    pending: Mutex<Vec<TransactionRecord>>,
}

impl DatabaseTransactionsManager {
    fn records(&mut self) -> &mut Vec<TransactionRecord> {
        self.pending
            .get_mut()
            .expect("exclusive transaction records")
    }

    fn record_at(&mut self, level: u32) -> &mut TransactionRecord {
        let records = self.records();
        if records.last().is_none_or(|record| record.level != level) {
            records.push(TransactionRecord {
                level,
                ..Default::default()
            });
        }
        records
            .last_mut()
            .expect("transaction record just inserted")
    }

    pub(super) fn add(&mut self, level: u32, callback: TransactionCallback) -> DatabaseResult<()> {
        if level == 0 {
            return Err(DatabaseError::TransactionClosed);
        }
        self.record_at(level).callbacks.push(callback);
        Ok(())
    }

    pub(super) fn add_at_level(&mut self, level: u32, callback: TransactionCallback) {
        let records = self.records();
        let index = records.partition_point(|record| record.level < level);
        if records
            .get(index)
            .is_none_or(|record| record.level != level)
        {
            records.insert(
                index,
                TransactionRecord {
                    level,
                    ..Default::default()
                },
            );
        }
        records[index].callbacks.push(callback);
    }

    pub(super) fn add_rollback(
        &mut self,
        level: u32,
        callback: TransactionCallback,
    ) -> DatabaseResult<()> {
        if level == 0 {
            return Err(DatabaseError::TransactionClosed);
        }
        self.record_at(level).rollback_callbacks.push(callback);
        Ok(())
    }

    pub(super) fn discard(&mut self) {
        self.records().clear();
    }

    pub(super) async fn rollback(&mut self, level: u32) -> DatabaseResult<()> {
        let records = self.records();
        let mut discarded =
            records.split_off(records.partition_point(|record| record.level <= level));
        let mut callbacks = Vec::new();
        if level == 0 {
            let mut staged = Vec::new();
            for record in &mut discarded {
                for child in std::mem::take(&mut record.committed_children) {
                    child.stage(&mut staged);
                }
            }
            staged.extend(discarded.into_iter().rev());
            staged.sort_by_key(|record| Reverse(record.level));
            for mut record in staged {
                callbacks.append(&mut record.rollback_callbacks);
            }
        } else {
            for record in discarded.into_iter().rev() {
                record.collect_rollback_callbacks(&mut callbacks);
            }
        }
        for callback in callbacks {
            callback().await.map_err(|source| match source {
                DatabaseError::AfterRollback { .. } => source,
                source => DatabaseError::AfterRollback {
                    source: Box::new(source),
                },
            })?;
        }
        Ok(())
    }

    pub(super) async fn commit(&mut self, level: u32) -> DatabaseResult<()> {
        let records = self.records();
        if records.last().is_none_or(|record| record.level != level) {
            return Ok(());
        }
        let record = records.pop().expect("active transaction record");
        if level > 1 {
            self.record_at(level - 1).committed_children.push(record);
        } else {
            let mut staged = Vec::new();
            record.stage(&mut staged);
            for callback in staged.into_iter().flat_map(|record| record.callbacks) {
                callback().await.map_err(|source| match source {
                    DatabaseError::AfterCommit { .. } => source,
                    source => DatabaseError::AfterCommit {
                        source: Box::new(source),
                    },
                })?;
            }
        }
        Ok(())
    }
}

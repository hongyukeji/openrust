use std::sync::Mutex;

use super::{DatabaseError, DatabaseFuture, DatabaseResult};

type CommitCallback = Box<dyn FnOnce() -> DatabaseFuture<'static, DatabaseResult<()>> + Send>;

#[derive(Default)]
struct TransactionRecord {
    level: u32,
    callbacks: Vec<CommitCallback>,
    committed_children: Vec<CommitCallback>,
}

/// Associates commit callbacks with one database transaction's savepoints.
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

    pub(super) fn add(&mut self, level: u32, callback: CommitCallback) -> DatabaseResult<()> {
        if level == 0 {
            return Err(DatabaseError::TransactionClosed);
        }
        self.record_at(level).callbacks.push(callback);
        Ok(())
    }

    pub(super) fn rollback(&mut self, level: u32) {
        let records = self.records();
        records.truncate(records.partition_point(|record| record.level <= level));
    }

    pub(super) async fn commit(&mut self, level: u32) -> DatabaseResult<()> {
        let records = self.records();
        if records.last().is_none_or(|record| record.level != level) {
            return Ok(());
        }
        let record = records.pop().expect("active transaction record");
        let callbacks = record
            .committed_children
            .into_iter()
            .chain(record.callbacks);
        if level > 1 {
            self.record_at(level - 1)
                .committed_children
                .extend(callbacks);
        } else {
            for callback in callbacks {
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

use openrust::database::{
    Transaction,
    query::{Builder, Expression, UpdateValue},
};

use crate::{
    DeliveryStore, Error, IdempotencyKind, Result,
    delivery_store::{check_id, text},
    idempotency::KEYS,
};

#[derive(Debug, serde::Serialize)]
pub struct IdempotencyPruneReport {
    pub kind: IdempotencyKind,
    pub candidates: usize,
    pub expired: u64,
    pub pretend: bool,
}

impl DeliveryStore {
    /// 过期的是幂等键。已完成记录只标记期限，供原 Worker 与诊断继续按 ID 定位。
    pub async fn prune_idempotency_keys(
        &self,
        kind: IdempotencyKind,
        days: u32,
        limit: u32,
        pretend: bool,
    ) -> Result<IdempotencyPruneReport> {
        if days > 36_500 || !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("prune.days/limit"));
        }
        let seconds = i64::from(days) * 86_400;
        let table = kind.table();
        let rows = self
            .idempotency_eligible(self.connection.table(table), kind, seconds)
            .use_write_pdo()
            .select(["id"])
            .order_by(kind.pruned_at(), "asc")
            .order_by("id", "asc")
            .limit(u64::from(limit))
            .get()
            .await?;
        let mut report = IdempotencyPruneReport {
            kind,
            candidates: rows.len(),
            expired: 0,
            pretend,
        };
        if !pretend {
            for row in &rows {
                let id = text(row, "id")?;
                check_id(id)?;
                let binding = self
                    .connection
                    .table(KEYS)
                    .use_write_pdo()
                    .r#where("scope_key", "=", self.scope_key.as_str())
                    .r#where("kind", "=", kind.name())
                    .r#where("record_id", "=", id)
                    .select(["id"])
                    .first()
                    .await?;
                let key = binding.as_ref().map(|row| text(row, "id")).transpose()?;
                report.expired += u64::from(
                    self.expire_idempotency_record(kind, id, key, seconds)
                        .await?,
                );
            }
        }
        Ok(report)
    }

    fn idempotency_eligible<'a>(
        &self,
        query: Builder<'a>,
        kind: IdempotencyKind,
        seconds: i64,
    ) -> Builder<'a> {
        let table = kind.table();
        query
            .r#where(&format!("{table}.scope_key"), "=", self.scope_key.as_str())
            .where_null(&format!("{table}.idempotency_expired_at"))
            .where_null(&format!("{table}.retention_hold_at"))
            .where_raw(
                &format!("{} < {} - ?", kind.pruned_at(), self.clock()),
                vec![seconds.into()],
            )
    }

    async fn expire_idempotency_record(
        &self,
        kind: IdempotencyKind,
        record: &str,
        binding: Option<&str>,
        seconds: i64,
    ) -> Result<bool> {
        let key = binding.map_or_else(|| kind.key_id(record), str::to_owned);
        check_id(&key)?;
        let mut tx = self.connection.begin_transaction().await?;
        let result = async {
            // 对没有映射的旧记录，先占据同一键槽再清理，防止旧键恰好被采用。
            if binding.is_none() {
                tx.table(KEYS)
                    .upsert(
                        &[[
                            ("id".into(), key.as_str().into()),
                            ("scope_key".into(), self.scope_key.as_str().into()),
                            ("kind".into(), kind.name().into()),
                            ("record_id".into(), record.into()),
                        ]
                        .into_iter()
                        .collect()],
                        &["id"],
                        Some(&["id"]),
                    )
                    .await?;
            }
            if self
                .delete_idempotency_binding(&mut tx, kind, &key, record)
                .await?
                != 1
            {
                return Ok(false);
            }
            let marked = self
                .idempotency_eligible(tx.table(kind.table()), kind, seconds)
                .r#where("id", "=", record)
                .update_with(
                    [(
                        "idempotency_expired_at".into(),
                        UpdateValue::Operand(Expression::new(self.clock()).into()),
                    )]
                    .into_iter()
                    .collect(),
                )
                .await?;
            Ok(marked == 1)
        }
        .await;
        match result {
            Ok(true) => {
                tx.commit().await?;
                Ok(true)
            }
            other => {
                tx.roll_back(None).await?;
                other
            }
        }
    }

    async fn delete_idempotency_binding(
        &self,
        tx: &mut Transaction,
        kind: IdempotencyKind,
        key: &str,
        record: &str,
    ) -> Result<u64> {
        Ok(tx
            .table(KEYS)
            .r#where("id", "=", key)
            .r#where("kind", "=", kind.name())
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("record_id", "=", record)
            .delete()
            .await?)
    }
}

impl IdempotencyKind {
    pub(crate) fn pruned_at(self) -> &'static str {
        match self {
            Self::Delivery => "payload_pruned_at",
            Self::Publication => "body_pruned_at",
        }
    }
}

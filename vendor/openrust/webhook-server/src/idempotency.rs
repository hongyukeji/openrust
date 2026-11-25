use openrust::{database::Transaction, support::Str};

use crate::{
    DeliveryStore, Error, Result,
    delivery_store::{DELIVERIES, check_id, hash, text},
    publication::MESSAGES,
};

pub(crate) const KEYS: &str = "webhook_idempotency_keys";

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdempotencyKind {
    Delivery,
    Publication,
}

impl IdempotencyKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Delivery => "delivery",
            Self::Publication => "publication",
        }
    }

    pub(crate) fn key_id(self, legacy_id: &str) -> String {
        hash(format!("webhook-idempotency:{}:{legacy_id}", self.name()).as_bytes())
    }

    pub(crate) fn table(self) -> &'static str {
        match self {
            Self::Delivery => DELIVERIES,
            Self::Publication => MESSAGES,
        }
    }
}

pub(crate) struct KeyBinding {
    pub id: String,
    pub new_record: bool,
}

impl DeliveryStore {
    /// 与投递或发布使用同一事务。旧键首次重试时沿用原记录。
    pub(crate) async fn bind_idempotency_key(
        &self,
        tx: &mut Transaction,
        kind: IdempotencyKind,
        legacy_id: &str,
    ) -> Result<KeyBinding> {
        let name = kind.name();
        let key = kind.key_id(legacy_id);
        let random = Str::uuid().map_err(|_| Error::InvalidRecord)?;
        let id = hash(format!("webhook-record:{name}:{random}").as_bytes());
        let inserted = tx
            .table(KEYS)
            .insert_or_ignore(&[[
                ("id".into(), key.as_str().into()),
                ("scope_key".into(), self.scope_key.as_str().into()),
                ("kind".into(), name.into()),
                ("record_id".into(), id.as_str().into()),
            ]
            .into_iter()
            .collect()])
            .await?;
        if inserted == 0 {
            let row = tx
                .table(KEYS)
                .r#where("scope_key", "=", self.scope_key.as_str())
                .r#where("kind", "=", name)
                .r#where("id", "=", key.as_str())
                .select(["record_id"])
                .shared_lock()
                .first()
                .await?
                .ok_or(Error::InvalidRecord)?;
            let id = text(&row, "record_id")?;
            check_id(id)?;
            return Ok(KeyBinding {
                id: id.to_owned(),
                new_record: false,
            });
        }

        let existing = tx
            .table(kind.table())
            .r#where("scope_key", "=", self.scope_key.as_str())
            .r#where("id", "=", legacy_id)
            .where_null("idempotency_expired_at")
            .select(["id"])
            .shared_lock()
            .first()
            .await?;
        if existing.is_some() {
            if tx
                .table(KEYS)
                .r#where("scope_key", "=", self.scope_key.as_str())
                .r#where("kind", "=", name)
                .r#where("id", "=", key.as_str())
                .update(
                    [("record_id".into(), legacy_id.into())]
                        .into_iter()
                        .collect(),
                )
                .await?
                != 1
            {
                return Err(Error::InvalidRecord);
            }
            return Ok(KeyBinding {
                id: legacy_id.to_owned(),
                new_record: false,
            });
        }
        Ok(KeyBinding {
            id,
            new_record: true,
        })
    }
}
